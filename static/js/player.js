// player.js - Part 1: Initialization and Setup

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
let maxReconnectAttempts = 15; // Increased max attempts
let connectionTimeout = null;
let checkNowPlayingInterval = null;
let lastAudioChunkTime = Date.now();
let debugMode = false; // Reduce console spam in production

// State tracking
let currentTrackId = null;
let lastKnownPosition = 0;
let connectionHealthTimer = null;
let lastErrorTime = 0;
let consecutiveErrors = 0;

// Buffer management constants - centralized configuration
const TARGET_BUFFER_SIZE = 10; // Target buffer duration in seconds
const MIN_BUFFER_SIZE = 3;     // Minimum buffer before playback starts
const MAX_BUFFER_SIZE = 30;    // Maximum buffer size in seconds
const BUFFER_MONITOR_INTERVAL = 3000; // Check buffer every 3 seconds
const NO_DATA_TIMEOUT = 20;   // Timeout for no data in seconds (increased from 15)
const AUDIO_STARVATION_THRESHOLD = 2; // Seconds of buffer left before action needed

// Format time (seconds to MM:SS)
function formatTime(seconds) {
    if (!seconds) return '0:00';
    const minutes = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

// Enhanced logging with timestamps and categories
function log(message, category = 'INFO', isError = false) {
    if (isError || debugMode) {
        const timestamp = new Date().toISOString().substr(11, 8);
        console[isError ? 'error' : 'log'](`[${timestamp}] [${category}] ${message}`);
    }
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
    log(`Status: ${message}`, 'UI', isError);
    
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

// Get current buffer health metrics
function getBufferHealth() {
    if (!sourceBuffer || !audioElement || sourceBuffer.buffered.length === 0) {
        return {
            current: 0,
            ahead: 0,
            duration: 0,
            underflow: true
        };
    }
    
    const currentTime = audioElement.currentTime;
    const bufferedEnd = sourceBuffer.buffered.end(sourceBuffer.buffered.length - 1);
    const bufferAhead = bufferedEnd - currentTime;
    const totalBuffered = sourceBuffer.buffered.end(sourceBuffer.buffered.length - 1) - 
                         sourceBuffer.buffered.start(0);
    
    return {
        current: currentTime,
        ahead: bufferAhead,
        duration: totalBuffered,
        underflow: bufferAhead < AUDIO_STARVATION_THRESHOLD
    };
}

// Improved process queue with smarter buffer management strategies
function processQueue() {
    // Exit conditions - ensure all necessary components are ready
    if (audioQueue.length === 0 || !sourceBuffer || !mediaSource || 
        mediaSource.readyState !== 'open' || sourceBuffer.updating) {
        return;
    }
    
    // Check buffer health before processing
    const bufferHealth = getBufferHealth();
    const queueSizeInChunks = audioQueue.length;
    
    // If we have a very high buffer, slow down processing
    if (bufferHealth.ahead > TARGET_BUFFER_SIZE * 1.5 && queueSizeInChunks > 5) {
        // We have plenty of buffer, so delay processing to avoid excess memory usage
        setTimeout(processQueue, 100);
        return;
    }
    
    try {
        // Get data from queue
        const data = audioQueue.shift();
        sourceBuffer.appendBuffer(data);
        lastAudioChunkTime = Date.now();
        
        // Reset consecutive errors since we successfully processed data
        consecutiveErrors = 0;
        
        // Log buffer status occasionally
        if (queueSizeInChunks % 50 === 0 || bufferHealth.underflow) {
            log(`Buffer health: ${bufferHealth.ahead.toFixed(1)}s ahead, ${queueSizeInChunks} chunks queued`, 'BUFFER');
        }
        
        // Set up callback for when this append completes
        sourceBuffer.addEventListener('updateend', function onUpdateEnd() {
            sourceBuffer.removeEventListener('updateend', onUpdateEnd);
            
            // Continue processing queue with adaptive scheduling
            if (audioQueue.length > 0) {
                // Adjust timing based on buffer health
                if (bufferHealth.ahead < MIN_BUFFER_SIZE) {
                    // Buffer is low, process next chunk immediately
                    processQueue();
                } else {
                    // Normal processing with a small delay to reduce CPU load
                    setTimeout(processQueue, 5);  // Small delay instead of 0
                }
            }
        }, { once: true });
        
    } catch (e) {
        log(`Error processing audio data: ${e.message}`, 'BUFFER', true);
        consecutiveErrors++;
        
        // Handle different error types
        if (e.name === 'QuotaExceededError') {
            // More strategic buffer management for quota errors
            handleQuotaExceededError();
        } else {
            // For other errors, try again soon with backoff
            const retryDelay = Math.min(50 * consecutiveErrors, 1000);
            setTimeout(processQueue, retryDelay);
            
            // If we've had many consecutive errors, try recreation
            if (consecutiveErrors > 5) {
                log('Too many consecutive errors, recreating MediaSource', 'BUFFER', true);
                recreateMediaSource();
            }
        }
    }
}

// Handle quota exceeded errors with smarter buffer management
function handleQuotaExceededError() {
    try {
        if (sourceBuffer && sourceBuffer.buffered.length > 0) {
            const currentTime = audioElement.currentTime;
            
            // Only remove data that's definitely been played
            const safeRemovalPoint = Math.max(
                sourceBuffer.buffered.start(0),
                currentTime - 2  // Keep 2 seconds before current position
            );
            
            // Calculate how much we need to remove
            const removalEnd = Math.min(
                safeRemovalPoint + 5,  // Remove 5 seconds of audio
                currentTime - 1  // But never too close to current playback position
            );
            
            if (removalEnd > safeRemovalPoint) {
                log(`Clearing buffer segment ${safeRemovalPoint.toFixed(1)}-${removalEnd.toFixed(1)}s`, 'BUFFER');
                sourceBuffer.remove(safeRemovalPoint, removalEnd);
                
                // Continue after buffer clear
                sourceBuffer.addEventListener('updateend', function onClearEnd() {
                    sourceBuffer.removeEventListener('updateend', onClearEnd);
                    setTimeout(processQueue, 50);
                }, { once: true });
                return;
            }
        }
        
        // If we couldn't clear the buffer using the approach above
        log('Could not clear buffer, trying alternative approach', 'BUFFER', true);
        recreateMediaSource();
    } catch (clearError) {
        log(`Error clearing buffer: ${clearError.message}`, 'BUFFER', true);
        recreateMediaSource();
    }
}

// Recreate the MediaSource to recover from serious errors
function recreateMediaSource() {
    log('Recreating MediaSource', 'MEDIA');
    
    try {
        // Preserve some audio data to continue playback
        const savedQueue = audioQueue.slice(-50); // Keep only the last 50 chunks
        audioQueue = []; // Clear the queue
        
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
            log('New MediaSource opened', 'MEDIA');
            
            try {
                // Create source buffer
                sourceBuffer = mediaSource.addSourceBuffer('audio/mpeg');
                
                // Setup error handler
                sourceBuffer.addEventListener('error', (event) => {
                    log(`SourceBuffer error: ${event.message || 'Unknown error'}`, 'MEDIA', true);
                });
                
                // Restore queue and continue
                audioQueue = savedQueue;
                consecutiveErrors = 0;
                setTimeout(processQueue, 100);
            } catch (e) {
                log(`Error creating source buffer: ${e.message}`, 'MEDIA', true);
                attemptReconnection();
            }
        });
        
        // Connect to audio element
        const url = URL.createObjectURL(mediaSource);
        audioElement.src = url;
        
        // Make sure we're playing
        if (audioElement.paused && isPlaying) {
            audioElement.play().catch(e => {
                log(`Error playing after recreation: ${e.message}`, 'MEDIA', true);
            });
        }
    } catch (e) {
        log(`Error recreating MediaSource: ${e.message}`, 'MEDIA', true);
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
                    log('Track transition marker received', 'STREAM');
                    // For track transitions, just keep processing
                    return;
                }
                if (view[0] === 0xFF && view[1] === 0xFF) {
                    log('Track end marker received', 'STREAM');
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
            log(`Error processing binary data: ${e.message}`, 'STREAM', true);
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
                log(`Track changed to: ${info.title}`, 'TRACK');
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
            log(`Non-JSON message: ${event.data}`, 'STREAM');
        }
    }
}

// Improved connection health monitoring
function checkConnectionHealth() {
    if (!isPlaying) return;
    
    const now = Date.now();
    const timeSinceLastAudio = (now - lastAudioChunkTime) / 1000;
    
    // Get buffer metrics
    const bufferHealth = getBufferHealth();
    
    // Check if we've received audio data recently
    if (timeSinceLastAudio > NO_DATA_TIMEOUT) {
        log(`No audio data received for ${timeSinceLastAudio.toFixed(1)}s`, 'HEALTH', true);
        
        // If buffer is also getting low, we need to reconnect
        if (bufferHealth.ahead < AUDIO_STARVATION_THRESHOLD) {
            log('Buffer depleted and no new data, reconnecting', 'HEALTH', true);
            showStatus('Connection interrupted. Reconnecting...', true, false);
            attemptReconnection();
        } else {
            // We still have buffer, so playback can continue
            // Send a ping to see if connection is still alive
            if (ws && ws.readyState === WebSocket.OPEN) {
                try {
                    ws.send('ping');
                    log('Sent ping to check connection', 'HEALTH');
                } catch (e) {
                    log(`Error sending ping: ${e.message}`, 'HEALTH', true);
                }
            }
        }
    } else {
        // Log buffer state for monitoring
        if (bufferHealth.ahead < AUDIO_STARVATION_THRESHOLD) {
            log(`WARNING: Low buffer - ${bufferHealth.ahead.toFixed(1)}s ahead, ${audioQueue.length} chunks queued`, 'HEALTH');
        } else if (debugMode) {
            log(`Buffer health: ${bufferHealth.ahead.toFixed(1)}s ahead, ${audioQueue.length} chunks queued`, 'HEALTH');
        }
    }
}

// player.js - Part 3: Connection and Playback Control

// Initialize and start playback with improved setup
function startAudio() {
    log('Starting audio playback', 'CONTROL');
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
        // Add to document but hide visually
        audioElement.style.display = 'none';
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
    connectionHealthTimer = setInterval(checkConnectionHealth, BUFFER_MONITOR_INTERVAL);
    
    // Start now playing updates
    if (checkNowPlayingInterval) {
        clearInterval(checkNowPlayingInterval);
    }
    checkNowPlayingInterval = setInterval(updateNowPlaying, 2000);
}

// Set up audio element event listeners
function setupAudioListeners() {
    audioElement.addEventListener('playing', () => {
        log('Audio playing', 'AUDIO');
        showStatus('Audio playing');
    });
    
    audioElement.addEventListener('waiting', () => {
        log('Audio buffering', 'AUDIO');
        showStatus('Buffering...', false, false);
    });
    
    audioElement.addEventListener('stalled', () => {
        log('Audio stalled', 'AUDIO');
        showStatus('Stream stalled - buffering', true, false);
    });
    
    audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        log(`Audio error (code ${errorCode})`, 'AUDIO', true);
        
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
        log('Audio ended', 'AUDIO');
        // If we shouldn't be at the end, try to restart
        if (isPlaying) {
            log('Audio ended unexpectedly, attempting to recover', 'AUDIO', true);
            showStatus('Audio ended - reconnecting', true, false);
            attemptReconnection();
        }
    });
    
    // Add new timeupdate listener to monitor buffer health dynamically
    audioElement.addEventListener('timeupdate', () => {
        // Check buffer health on time updates (but not too frequently - skip most updates)
        if (Math.random() < 0.05) { // Only check ~5% of time updates to reduce overhead
            const bufferHealth = getBufferHealth();
            if (bufferHealth.underflow) {
                log(`Buffer underfull during playback: ${bufferHealth.ahead.toFixed(2)}s ahead`, 'AUDIO');
                // Process queue immediately if we have data
                if (audioQueue.length > 0 && !sourceBuffer.updating) {
                    processQueue();
                }
            }
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
            log('MediaSource opened', 'MEDIA');
            
            try {
                // Create source buffer for MP3
                sourceBuffer = mediaSource.addSourceBuffer('audio/mpeg');
                
                // Add buffer monitoring event
                sourceBuffer.addEventListener('updateend', () => {
                    // Check how much we've buffered after each update
                    if (sourceBuffer && sourceBuffer.buffered.length > 0 && audioElement) {
                        const bufferHealth = getBufferHealth();
                        
                        // If buffer is getting very large, trim it
                        if (bufferHealth.duration > MAX_BUFFER_SIZE) {
                            const currentTime = audioElement.currentTime;
                            const trimPoint = Math.max(sourceBuffer.buffered.start(0), currentTime - 10);
                            log(`Trimming buffer: ${trimPoint.toFixed(2)}s to current time - 10`, 'BUFFER');
                            try {
                                sourceBuffer.remove(sourceBuffer.buffered.start(0), trimPoint);
                            } catch (e) {
                                log(`Error trimming buffer: ${e.message}`, 'BUFFER');
                            }
                        }
                    }
                });
                
                // Connect to WebSocket after MediaSource is ready
                connectWebSocket();
            } catch (e) {
                log(`Error creating source buffer: ${e.message}`, 'MEDIA', true);
                showStatus(`Media error: ${e.message}`, true);
                startBtn.disabled = false;
            }
        });
        
        mediaSource.addEventListener('sourceended', () => log('MediaSource ended', 'MEDIA'));
        mediaSource.addEventListener('sourceclose', () => log('MediaSource closed', 'MEDIA'));
        
        // Create object URL and set as audio source
        const url = URL.createObjectURL(mediaSource);
        audioElement.src = url;
        
    } catch (e) {
        log(`MediaSource setup error: ${e.message}`, 'MEDIA', true);
        showStatus(`Media error: ${e.message}`, true);
        startBtn.disabled = false;
    }
}

// Improved WebSocket connection with better error handling
function connectWebSocket() {
    // Clean up any existing connection
    if (ws) {
        try {
            ws.close();
        } catch (e) {
            // Ignore close errors
        }
        ws = null;
    }
    
    try {
        // Determine WebSocket URL
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/stream`;
        log(`Connecting to WebSocket: ${wsUrl}`, 'STREAM');
        
        // Create connection
        ws = new WebSocket(wsUrl);
        
        // Set up event handlers
        ws.onopen = () => {
            log('WebSocket connection established', 'STREAM');
            showStatus('Connected to stream');
            startBtn.textContent = 'Disconnect';
            startBtn.disabled = false;
            startBtn.dataset.connected = 'true';
            isPlaying = true;
            
            // Reset reconnect attempts on successful connection
            reconnectAttempts = 0;
            
            // Start audio playback
            if (audioElement.paused) {
                const playPromise = audioElement.play();
                playPromise.catch(e => {
                    log(`Play error: ${e.message}`, 'AUDIO', true);
                    if (e.name === 'NotAllowedError') {
                        showStatus('Click play to start audio (browser requires user interaction)', true, false);
                    }
                });
            }
        };
        
        ws.onclose = (event) => {
            log(`WebSocket closed: Code ${event.code}`, 'STREAM');
            
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
        
        ws.onerror = (error) => {
            log('WebSocket error', 'STREAM', true);
            
            // Don't immediately try to reconnect - wait for the close event
            showStatus('Connection error', true, false);
        };
        
        ws.onmessage = handleWebSocketMessage;
        
        // Set connection timeout (increased for slower connections)
        connectionTimeout = setTimeout(() => {
            if (ws && audioQueue.length === 0) {
                log('Connection timeout - no data received', 'STREAM', true);
                showStatus('Connection timeout. Reconnecting...', true, false);
                attemptReconnection();
            }
        }, 20000); // Increased from 15s to 20s
        
    } catch (e) {
        log(`WebSocket setup error: ${e.message}`, 'STREAM', true);
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
        log(`Maximum reconnection attempts (${maxReconnectAttempts}) reached`, 'CONTROL', true);
        showStatus('Could not reconnect to server. Please try again later.', true);
        
        // Reset UI
        stopAudio(true);
        return;
    }
    
    // Increment attempts
    reconnectAttempts++;
    
    // Calculate delay with exponential backoff and a bit of randomness
    // More gradual backoff with smaller initial delays
    const baseDelay = Math.min(500 * Math.pow(1.3, reconnectAttempts - 1), 8000);
    const jitter = Math.random() * 1000; // Add up to 1 second of jitter
    const delay = baseDelay + jitter;
    
    log(`Reconnection attempt ${reconnectAttempts}/${maxReconnectAttempts} in ${(delay/1000).toFixed(1)}s`, 'CONTROL');
    showStatus(`Reconnecting (${reconnectAttempts}/${maxReconnectAttempts})...`, true, false);
    
    // Close existing connection
    if (ws) {
        try {
            ws.close();
        } catch (e) {
            // Ignore close errors
        }
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
    log(`Stopping audio playback${isError ? ' (due to error)' : ''}`, 'CONTROL');
    
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
        try {
            ws.close();
        } catch (e) {
            // Ignore close errors
        }
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
            log(`Error ending MediaSource: ${e.message}`, 'MEDIA');
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

// player.js - Part 4: UI Control and API Integration

// Toggle connection
function toggleConnection() {
    const isConnected = startBtn.dataset.connected === 'true';
    
    if (isConnected) {
        log('User requested disconnect', 'CONTROL');
        stopAudio();
    } else {
        log('User requested connect', 'CONTROL');
        startAudio();
    }
}

// Update now playing information
async function updateNowPlaying() {
    if (!isPlaying) return;
    
    try {
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            log(`Now playing API error: ${response.status}`, 'API', true);
            return;
        }
        
        const data = await response.json();
        
        if (data.error) {
            log(`Now playing error: ${data.error}`, 'API', true);
            currentTitle.textContent = 'No tracks available';
            currentArtist.textContent = 'Please add MP3 files to the server';
            currentAlbum.textContent = '';
            return;
        }
        
        // Store track ID for change detection
        const newTrackId = data.path;
        const trackChanged = currentTitle.dataset.trackId !== newTrackId;
        
        if (trackChanged) {
            log(`Track info changed to: "${data.title}" by "${data.artist}"`, 'TRACK');
            
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
        log(`Error fetching now playing: ${error.message}`, 'API', true);
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