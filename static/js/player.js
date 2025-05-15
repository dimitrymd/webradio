// player.js with improved connection stability and resilience

// Elements
const startBtn = document.getElementById('start-btn');
const muteBtn = document.getElementById('mute-btn');
const volumeControl = document.getElementById('volume');
const statusMessage = document.getElementById('status-message');
const listenerCount = document.getElementById('listener-count');

// Current track display elements
const currentTitle = document.getElementById('current-title');
const currentArtist = document.getElementById('current-artist');
const currentAlbum = document.getElementById('current-album');
const currentPosition = document.getElementById('current-position');
const currentDuration = document.getElementById('current-duration');
const progressBar = document.getElementById('progress-bar');

// WebSocket and audio context
let ws = null;
let audioElement = null;
let mediaSource = null;
let sourceBuffer = null;
let audioQueue = [];
let isPlaying = false;
let isMuted = false;
let reconnectAttempts = 0;
let maxReconnectAttempts = 10; // Increased max attempts
let connectionTimeout = null;
let checkNowPlayingInterval = null;
let lastAudioChunkTime = Date.now();
let debugMode = true;

// State tracking
let currentTrackId = null;
let lastKnownPosition = 0;
let connectionHealthTimer = null;
let lastErrorTime = 0;
let consecutiveErrors = 0;

// Format time (seconds to MM:SS)
function formatTime(seconds) {
    if (!seconds) return '0:00';
    const minutes = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

// Update the progress bar
function updateProgressBar(position, duration) {
    if (progressBar && duration > 0) {
        const percent = (position / duration) * 100;
        progressBar.style.width = `${percent}%`;
        
        // Update text display
        if (currentPosition) currentPosition.textContent = formatTime(position);
        if (currentDuration) currentDuration.textContent = formatTime(duration);
    }
}

// Show status message with optional auto-hide
function showStatus(message, isError = false, autoHide = true) {
    console.log(`Status: ${message}${isError ? ' (ERROR)' : ''}`);
    
    statusMessage.textContent = message;
    statusMessage.style.display = 'block';
    statusMessage.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
    
    // Hide after 3 seconds for non-errors if autoHide is true
    if (!isError && autoHide) {
        setTimeout(() => {
            statusMessage.style.display = 'none';
        }, 3000);
    }
}

// Process the audio queue with improved error handling
function processQueue() {
    // Don't process if no data, not initialized, or already updating
    if (audioQueue.length === 0 || !sourceBuffer || !mediaSource || 
        mediaSource.readyState !== 'open' || sourceBuffer.updating) {
        return;
    }
    
    try {
        // Get data from queue
        const data = audioQueue.shift();
        sourceBuffer.appendBuffer(data);
        lastAudioChunkTime = Date.now();
        
        // Reset consecutive errors since we successfully processed data
        consecutiveErrors = 0;
        
        // Set up callback for when this append completes
        sourceBuffer.addEventListener('updateend', function onUpdateEnd() {
            sourceBuffer.removeEventListener('updateend', onUpdateEnd);
            
            // Continue processing queue
            setTimeout(processQueue, 0);
        }, { once: true });
        
    } catch (e) {
        console.error(`Error processing audio data: ${e.message}`);
        consecutiveErrors++;
        
        // Handle quota exceeded errors
        if (e.name === 'QuotaExceededError') {
            try {
                if (sourceBuffer.buffered.length > 0) {
                    // Just remove a small portion of the buffer
                    const start = sourceBuffer.buffered.start(0);
                    const end = Math.min(
                        sourceBuffer.buffered.end(0), 
                        start + 10
                    );
                    
                    console.log(`Clearing buffer segment ${start}-${end}s`);
                    sourceBuffer.remove(start, end);
                    
                    // Put the data back in the queue
                    audioQueue.unshift(data);
                    
                    // Continue after buffer clear
                    sourceBuffer.addEventListener('updateend', function onClearEnd() {
                        sourceBuffer.removeEventListener('updateend', onClearEnd);
                        setTimeout(processQueue, 50);
                    }, { once: true });
                    return;
                }
            } catch (clearError) {
                console.error(`Error clearing buffer: ${clearError.message}`);
            }
        }
        
        // For other errors, try again soon, but don't retry too quickly
        // if we keep seeing errors
        const retryDelay = Math.min(100 * consecutiveErrors, 2000);
        setTimeout(processQueue, retryDelay);
        
        // If we've had too many consecutive errors, try recreating the MediaSource
        if (consecutiveErrors > 10) {
            console.warn('Too many consecutive errors, recreating MediaSource');
            recreateMediaSource();
        }
    }
}

// Recreate the MediaSource to recover from serious errors
function recreateMediaSource() {
    console.log('Recreating MediaSource');
    
    try {
        // Preserve some audio data to continue playback
        const savedQueue = [...audioQueue];
        
        // Clean up old MediaSource
        if (sourceBuffer) {
            sourceBuffer = null;
        }
        
        if (mediaSource && mediaSource.readyState === 'open') {
            try {
                mediaSource.endOfStream();
            } catch (e) {
                // Ignore errors during cleanup
            }
        }
        
        // Create new MediaSource
        mediaSource = new MediaSource();
        
        mediaSource.addEventListener('sourceopen', function onSourceOpen() {
            console.log('New MediaSource opened');
            
            try {
                // Create source buffer
                sourceBuffer = mediaSource.addSourceBuffer('audio/mpeg');
                
                // Restore queue and continue
                audioQueue = savedQueue;
                consecutiveErrors = 0;
                setTimeout(processQueue, 100);
            } catch (e) {
                console.error(`Error creating source buffer: ${e.message}`);
            }
        });
        
        // Connect to audio element
        const url = URL.createObjectURL(mediaSource);
        audioElement.src = url;
        
        // Make sure we're playing
        if (audioElement.paused && isPlaying) {
            audioElement.play().catch(e => {
                console.error(`Error playing after recreation: ${e.message}`);
            });
        }
    } catch (e) {
        console.error(`Error recreating MediaSource: ${e.message}`);
        // If recreation fails, attempt reconnection as a last resort
        attemptReconnection();
    }
}

// Handle WebSocket messages
function handleWebSocketMessage(event) {
    // Clear connection timeout if set
    if (connectionTimeout) {
        clearTimeout(connectionTimeout);
        connectionTimeout = null;
    }
    
    // Process binary audio data
    if (event.data instanceof Blob) {
        // Handle non-text data (audio)
        event.data.arrayBuffer().then(buffer => {
            // Check for special markers (2-byte control messages)
            if (buffer.byteLength === 2) {
                const view = new Uint8Array(buffer);
                if (view[0] === 0xFF && view[1] === 0xFE) {
                    console.log('Track transition marker received');
                    // For track transitions, just keep processing
                    return;
                }
                if (view[0] === 0xFF && view[1] === 0xFF) {
                    console.log('Track end marker received');
                    return;
                }
            }
            
            // Skip empty buffers
            if (buffer.byteLength === 0) {
                return;
            }
            
            // Add data to queue
            audioQueue.push(buffer);
            lastAudioChunkTime = Date.now();
            
            // Start processing if not already going
            processQueue();
            
        }).catch(e => {
            console.error(`Error processing binary data: ${e.message}`);
        });
    } else {
        // Process text data (track info)
        try {
            const info = JSON.parse(event.data);
            
            // Check for error message
            if (info.error) {
                showStatus(`Server error: ${info.error}`, true);
                return;
            }
            
            // Store track ID for change detection
            const newTrackId = info.path;
            if (currentTrackId !== newTrackId) {
                console.log(`Track changed to: ${info.title}`);
                currentTrackId = newTrackId;
                
                // Reset position tracking
                lastKnownPosition = 0;
            }
            
            // Update UI
            currentTitle.textContent = info.title || 'Unknown Title';
            currentArtist.textContent = info.artist || 'Unknown Artist';
            currentAlbum.textContent = info.album || 'Unknown Album';
            
            // Update progress
            if (info.duration) {
                currentDuration.textContent = formatTime(info.duration);
            }
            
            if (info.playback_position !== undefined) {
                lastKnownPosition = info.playback_position;
                updateProgressBar(info.playback_position, info.duration);
            }
            
            // Update listener count
            if (info.active_listeners !== undefined) {
                listenerCount.textContent = `Listeners: ${info.active_listeners}`;
            }
            
            // Store track ID in DOM for future comparison
            currentTitle.dataset.trackId = info.path;
            
            // Update page title
            document.title = `${info.title} - ${info.artist} | Rust Web Radio`;
        } catch (e) {
            console.log(`Non-JSON message: ${event.data}`);
        }
    }
}

// Check connection health periodically
function checkConnectionHealth() {
    if (!isPlaying) return;
    
    const now = Date.now();
    const timeSinceLastAudio = (now - lastAudioChunkTime) / 1000;
    
    // Check if we've received audio data recently
    if (timeSinceLastAudio > 15) { // 15 seconds without data is a problem
        console.warn(`No audio data received for ${timeSinceLastAudio.toFixed(1)}s`);
        
        // Check buffer health
        let bufferAhead = 0;
        if (sourceBuffer && sourceBuffer.buffered.length > 0 && !audioElement.paused) {
            const currentTime = audioElement.currentTime;
            const bufferedEnd = sourceBuffer.buffered.end(sourceBuffer.buffered.length - 1);
            bufferAhead = bufferedEnd - currentTime;
            
            console.log(`Buffer has ${bufferAhead.toFixed(1)}s of audio ahead`);
        }
        
        // If buffer is also getting low, we need to reconnect
        if (bufferAhead < 3) {
            console.warn('Buffer depleted and no new data, reconnecting');
            showStatus('Connection interrupted. Reconnecting...', true, false);
            attemptReconnection();
        } else {
            // We still have buffer, so playback can continue
            // Send a ping to see if connection is still alive
            if (ws && ws.readyState === WebSocket.OPEN) {
                try {
                    ws.send('ping');
                    console.log('Sent ping to check connection');
                } catch (e) {
                    console.error(`Error sending ping: ${e.message}`);
                }
            }
        }
    } else if (sourceBuffer && sourceBuffer.buffered.length > 0 && !audioElement.paused) {
        // Just log buffer state for debugging
        const currentTime = audioElement.currentTime;
        const bufferedEnd = sourceBuffer.buffered.end(sourceBuffer.buffered.length - 1);
        const bufferAhead = bufferedEnd - currentTime;
        
        console.log(`Health check: ${bufferAhead.toFixed(1)}s buffered, ${audioQueue.length} chunks queued`);
    }
}

// Initialize and start playback
function startAudio() {
    console.log('Starting audio playback');
    startBtn.disabled = true;
    showStatus('Connecting to stream...', false, false);
    
    // Reset state
    reconnectAttempts = 0;
    audioQueue = [];
    consecutiveErrors = 0;
    lastAudioChunkTime = Date.now();
    
    // Check browser support
    if (!('WebSocket' in window)) {
        showStatus('Your browser does not support WebSockets', true);
        startBtn.disabled = false;
        return;
    }
    
    if (!('MediaSource' in window)) {
        showStatus('Your browser does not support MediaSource', true);
        startBtn.disabled = false;
        return;
    }
    
    // Set up audio element
    if (!audioElement) {
        audioElement = new Audio();
        audioElement.controls = false;
        audioElement.volume = volumeControl.value;
        audioElement.muted = isMuted;
        audioElement.preload = 'auto';
        document.body.appendChild(audioElement);
        
        // Set up audio event listeners
        setupAudioListeners();
    }
    
    // Set up MediaSource
    setupMediaSource();
    
    // Start connection health check timer
    if (connectionHealthTimer) {
        clearInterval(connectionHealthTimer);
    }
    connectionHealthTimer = setInterval(checkConnectionHealth, 5000);
    
    // Start now playing updates
    if (checkNowPlayingInterval) {
        clearInterval(checkNowPlayingInterval);
    }
    checkNowPlayingInterval = setInterval(updateNowPlaying, 2000);
}

// Set up audio element event listeners
function setupAudioListeners() {
    audioElement.addEventListener('playing', () => {
        console.log('Audio playing');
        showStatus('Audio playing');
    });
    
    audioElement.addEventListener('waiting', () => {
        console.log('Audio buffering');
        showStatus('Buffering...', false, false);
    });
    
    audioElement.addEventListener('stalled', () => {
        console.log('Audio stalled');
        showStatus('Stream stalled - buffering', true, false);
    });
    
    audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        console.error(`Audio error (code ${errorCode})`);
        
        // Only react to errors if we're still trying to play
        if (isPlaying) {
            // Don't react to errors too frequently
            const now = Date.now();
            if (now - lastErrorTime > 10000) { // At most one error response per 10 seconds
                lastErrorTime = now;
                showStatus('Audio error - attempting to recover', true, false);
                
                // Try recreating the MediaSource
                recreateMediaSource();
            }
        }
    });
    
    audioElement.addEventListener('ended', () => {
        console.log('Audio ended');
        // If we shouldn't be at the end, try to restart
        if (isPlaying) {
            console.warn('Audio ended unexpectedly, attempting to recover');
            showStatus('Audio ended - reconnecting', true, false);
            attemptReconnection();
        }
    });
}

// Set up MediaSource with error handling
function setupMediaSource() {
    try {
        // Create MediaSource
        mediaSource = new MediaSource();
        
        // Set up event handlers
        mediaSource.addEventListener('sourceopen', () => {
            console.log('MediaSource opened');
            
            try {
                // Create source buffer for MP3
                sourceBuffer = mediaSource.addSourceBuffer('audio/mpeg');
                
                // Connect to WebSocket after MediaSource is ready
                connectWebSocket();
            } catch (e) {
                console.error(`Error creating source buffer: ${e.message}`);
                showStatus(`Media error: ${e.message}`, true);
                startBtn.disabled = false;
            }
        });
        
        mediaSource.addEventListener('sourceended', () => console.log('MediaSource ended'));
        mediaSource.addEventListener('sourceclose', () => console.log('MediaSource closed'));
        
        // Create object URL and set as audio source
        const url = URL.createObjectURL(mediaSource);
        audioElement.src = url;
        
    } catch (e) {
        console.error(`MediaSource setup error: ${e.message}`);
        showStatus(`Media error: ${e.message}`, true);
        startBtn.disabled = false;
    }
}

// Connect to WebSocket
function connectWebSocket() {
    // Clean up any existing connection
    if (ws) {
        ws.close();
        ws = null;
    }
    
    try {
        // Determine WebSocket URL
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/stream`;
        console.log(`Connecting to WebSocket: ${wsUrl}`);
        
        // Create connection
        ws = new WebSocket(wsUrl);
        
        // Set up event handlers
        ws.onopen = () => {
            console.log('WebSocket connection established');
            showStatus('Connected to stream');
            startBtn.textContent = 'Disconnect';
            startBtn.disabled = false;
            startBtn.dataset.connected = 'true';
            isPlaying = true;
            
            // Reset reconnect attempts on successful connection
            reconnectAttempts = 0;
            
            // Start audio playback
            if (audioElement.paused) {
                audioElement.play().catch(e => {
                    console.error(`Play error: ${e.message}`);
                    if (e.name === 'NotAllowedError') {
                        showStatus('Click play to start audio (browser requires user interaction)', true, false);
                    }
                });
            }
        };
        
        ws.onclose = (event) => {
            console.log(`WebSocket closed: Code ${event.code}`);
            
            // Only attempt reconnect if we're still supposed to be playing
            if (isPlaying) {
                // Use a brief delay to avoid hammering the server
                setTimeout(() => {
                    if (isPlaying) {
                        showStatus('Connection closed. Reconnecting...', true, false);
                        attemptReconnection();
                    }
                }, 1000);
            }
        };
        
        ws.onerror = () => {
            console.error('WebSocket error');
            
            // Don't immediately try to reconnect - wait for the close event
            showStatus('Connection error', true, false);
        };
        
        ws.onmessage = handleWebSocketMessage;
        
        // Set connection timeout
        connectionTimeout = setTimeout(() => {
            if (ws && audioQueue.length === 0) {
                console.error('Connection timeout - no data received');
                showStatus('Connection timeout. Reconnecting...', true, false);
                attemptReconnection();
            }
        }, 15000); // Longer timeout
        
    } catch (e) {
        console.error(`WebSocket setup error: ${e.message}`);
        showStatus(`Connection error: ${e.message}`, true);
        attemptReconnection();
    }
}

// Attempt reconnection with exponential backoff
function attemptReconnection() {
    // Don't try to reconnect if we're not supposed to be playing
    if (!isPlaying) return;
    
    // Check if we've reached the maximum attempts
    if (reconnectAttempts >= maxReconnectAttempts) {
        console.error(`Maximum reconnection attempts (${maxReconnectAttempts}) reached`);
        showStatus('Could not reconnect to server. Please try again later.', true);
        
        // Reset UI
        stopAudio(true);
        return;
    }
    
    // Increment attempts
    reconnectAttempts++;
    
    // Calculate delay with exponential backoff and a bit of randomness
    const baseDelay = Math.min(1000 * Math.pow(1.5, reconnectAttempts - 1), 10000);
    const jitter = Math.random() * 1000; // Add up to 1 second of jitter
    const delay = baseDelay + jitter;
    
    console.log(`Reconnection attempt ${reconnectAttempts}/${maxReconnectAttempts} in ${(delay/1000).toFixed(1)}s`);
    showStatus(`Reconnecting (${reconnectAttempts}/${maxReconnectAttempts})...`, true, false);
    
    // Close existing connection
    if (ws) {
        ws.close();
        ws = null;
    }
    
    // Schedule reconnection
    setTimeout(() => {
        if (isPlaying) {
            // Set up a fresh MediaSource
            setupMediaSource();
        }
    }, delay);
}

// Stop audio playback and disconnect
function stopAudio(isError = false) {
    console.log(`Stopping audio playback${isError ? ' (due to error)' : ''}`);
    
    isPlaying = false;
    
    // Clear all timers
    if (checkNowPlayingInterval) {
        clearInterval(checkNowPlayingInterval);
        checkNowPlayingInterval = null;
    }
    
    if (connectionHealthTimer) {
        clearInterval(connectionHealthTimer);
        connectionHealthTimer = null;
    }
    
    if (connectionTimeout) {
        clearTimeout(connectionTimeout);
        connectionTimeout = null;
    }
    
    // Close WebSocket
    if (ws) {
        ws.close();
        ws = null;
    }
    
    // Clean up MediaSource
    if (sourceBuffer) {
        sourceBuffer = null;
    }
    
    if (mediaSource && mediaSource.readyState === 'open') {
        try {
            mediaSource.endOfStream();
        } catch (e) {
            console.error(`Error ending MediaSource: ${e.message}`);
        }
    }
    mediaSource = null;
    
    // Stop audio
    if (audioElement) {
        audioElement.pause();
        audioElement.src = '';
        audioElement.load();
    }
    
    // Clear queue
    audioQueue = [];
    
    if (!isError) {
        showStatus('Disconnected from audio stream');
    }
    
    // Reset UI
    startBtn.textContent = 'Connect';
    startBtn.disabled = false;
    startBtn.dataset.connected = 'false';
}

// Toggle connection
function toggleConnection() {
    const isConnected = startBtn.dataset.connected === 'true';
    
    if (isConnected) {
        console.log('User requested disconnect');
        stopAudio();
    } else {
        console.log('User requested connect');
        startAudio();
    }
}

// Update now playing information
async function updateNowPlaying() {
    try {
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            console.error(`Now playing API error: ${response.status}`);
            return;
        }
        
        const data = await response.json();
        
        if (data.error) {
            console.error(`Now playing error: ${data.error}`);
            currentTitle.textContent = 'No tracks available';
            currentArtist.textContent = 'Please add MP3 files to the server';
            currentAlbum.textContent = '';
            return;
        }
        
        // Store track ID for change detection
        const newTrackId = data.path;
        const trackChanged = currentTitle.dataset.trackId !== newTrackId;
        
        if (trackChanged) {
            console.log(`Track info changed to: "${data.title}" by "${data.artist}"`);
            
            // Update track ID
            currentTitle.dataset.trackId = newTrackId;
            currentTrackId = newTrackId;
            
            // Reset position tracking
            lastKnownPosition = 0;
        }
        
        // Update display elements
        currentTitle.textContent = data.title || 'Unknown Title';
        currentArtist.textContent = data.artist || 'Unknown Artist';
        currentAlbum.textContent = data.album || 'Unknown Album';
        
        // Update progress info
        if (data.duration) {
            currentDuration.textContent = formatTime(data.duration);
        }
        
        if (data.playback_position !== undefined) {
            lastKnownPosition = data.playback_position;
            updateProgressBar(data.playback_position, data.duration);
        }
        
        // Update listener count if available
        if (data.active_listeners !== undefined) {
            listenerCount.textContent = `Listeners: ${data.active_listeners}`;
        }
        
        // Update page title
        document.title = `${data.title} - ${data.artist} | Rust Web Radio`;
    } catch (error) {
        console.error(`Error fetching now playing: ${error.message}`);
    }
}

// Volume control
volumeControl.addEventListener('input', function() {
    if (audioElement) {
        audioElement.volume = this.value;
    }
    
    // Save to local storage
    try {
        localStorage.setItem('radioVolume', this.value);
    } catch (e) {
        // Ignore storage errors
    }
});

// Mute button
muteBtn.addEventListener('click', function() {
    isMuted = !isMuted;
    
    if (audioElement) {
        audioElement.muted = isMuted;
    }
    
    muteBtn.textContent = isMuted ? 'Unmute' : 'Mute';
});

// Connect button
startBtn.addEventListener('click', toggleConnection);

// Load saved volume
try {
    const savedVolume = localStorage.getItem('radioVolume');
    if (savedVolume !== null) {
        volumeControl.value = savedVolume;
    }
} catch (e) {
    // Ignore storage errors
}

// Initialize by updating now playing
updateNowPlaying();