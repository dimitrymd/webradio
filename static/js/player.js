// Fixed player.js - resolves connection issues and song info display

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
let maxReconnectAttempts = 5;
let connectionTimeout = null;
let checkNowPlayingInterval = null;
let audioLastUpdateTime = Date.now();
let lastProgressUpdate = Date.now();
let maxQueueSize = 0;
let isProcessingQueue = false;
let debugMode = true; // Enable for troubleshooting

// Track current state for better recovery
let currentTrackId = null;
let lastKnownPosition = 0;

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

// Show status message
function showStatus(message, isError = false) {
    if (debugMode) {
        console.log(`Status: ${message}${isError ? ' (ERROR)' : ''}`);
    }
    
    statusMessage.textContent = message;
    statusMessage.style.display = 'block';
    statusMessage.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
    
    // Hide after 3 seconds for non-errors
    if (!isError) {
        setTimeout(() => {
            statusMessage.style.display = 'none';
        }, 3000);
    }
}

// Process queue
function processQueue() {
    if (audioQueue.length === 0 || isProcessingQueue || !sourceBuffer || sourceBuffer.updating) {
        isProcessingQueue = false;
        return;
    }
    
    isProcessingQueue = true;
    const data = audioQueue.shift();
    
    try {
        // If buffer is getting too large, remove old data
        if (sourceBuffer.buffered.length > 0) {
            const bufferedEnd = sourceBuffer.buffered.end(sourceBuffer.buffered.length - 1);
            const bufferedStart = sourceBuffer.buffered.start(0);
            const bufferedDuration = bufferedEnd - bufferedStart;
            const currentTime = audioElement.currentTime;
            
            // Remove data that's been played already to save memory
            if (bufferedDuration > 30 && currentTime > bufferedStart + 1) {
                // Keep a small margin before current playback position
                const removeEnd = Math.max(currentTime - 1, bufferedStart);
                
                if (removeEnd > bufferedStart) {
                    if (debugMode) console.log(`Trimming buffer ${bufferedStart.toFixed(2)}s to ${removeEnd.toFixed(2)}s`);
                    sourceBuffer.remove(bufferedStart, removeEnd);
                    
                    // Wait for remove to complete
                    sourceBuffer.addEventListener('updateend', function onRemoveEnd() {
                        sourceBuffer.removeEventListener('updateend', onRemoveEnd);
                        isProcessingQueue = false;
                        processQueue(); // Continue processing after removal
                    });
                    return;
                }
            }
        }
        
        // Append new data
        sourceBuffer.appendBuffer(data);
        
    } catch (e) {
        console.error(`Error appending buffer: ${e.name} - ${e.message}`);
        
        if (e.name === 'QuotaExceededError') {
            // Buffer is full, try to clear some space
            if (sourceBuffer.buffered.length > 0) {
                const bufferedStart = sourceBuffer.buffered.start(0);
                const currentTime = audioElement.currentTime;
                
                // Remove data that's too old
                const removeEnd = Math.max(currentTime - 1, bufferedStart + 1);
                
                if (removeEnd > bufferedStart) {
                    try {
                        sourceBuffer.remove(bufferedStart, removeEnd);
                        // Put the data back in queue
                        audioQueue.unshift(data);
                    } catch (removeError) {
                        console.error(`Error removing buffer: ${removeError.message}`);
                    }
                }
            }
        }
        
        isProcessingQueue = false;
    }
}

// Handle WebSocket messages
function handleWebSocketMessage(event) {
    // Clear connection timeout if set
    if (connectionTimeout) {
        clearTimeout(connectionTimeout);
        connectionTimeout = null;
    }
    
    // Reset the audioLastUpdateTime
    audioLastUpdateTime = Date.now();
    
    // Process binary audio data
    if (event.data instanceof Blob) {
        // Convert blob to array buffer
        event.data.arrayBuffer().then(buffer => {
            // Check for special markers
            if (buffer.byteLength === 2) {
                const view = new Uint8Array(buffer);
                
                // Track transition marker
                if (view[0] === 0xFF && view[1] === 0xFE) {
                    console.log('Track transition detected');
                    
                    // CRITICAL: Reset MediaSource for clean track transition
                    handleTrackTransition();
                    return;
                }
                
                // Track end marker
                if (view[0] === 0xFF && view[1] === 0xFF) {
                    console.log('Track end marker received');
                    return;
                }
            }
            
            // Handle empty buffer (track end or flush)
            if (buffer.byteLength === 0) {
                return;
            }
            
            // Process normal audio data
            if (sourceBuffer && mediaSource && mediaSource.readyState === 'open') {
                // Add to queue
                audioQueue.push(buffer);
                
                // Track maximum queue size for diagnostics
                if (audioQueue.length > maxQueueSize) {
                    maxQueueSize = audioQueue.length;
                    if (maxQueueSize % 10 === 0 && maxQueueSize > 30) {
                        console.log(`Queue size peak: ${maxQueueSize} chunks`);
                    }
                }
                
                // Process queue if not already processing
                if (!isProcessingQueue && !sourceBuffer.updating) {
                    processQueue();
                }
            } else {
                // If MediaSource isn't ready, we might need to recreate it
                if (!mediaSource || mediaSource.readyState !== 'open') {
                    console.log('MediaSource not ready for audio data, attempting recovery');
                    
                    // Store the audio data temporarily
                    if (!window.pendingAudioData) {
                        window.pendingAudioData = [];
                    }
                    window.pendingAudioData.push(buffer);
                    
                    // Try to reinitialize MediaSource
                    reinitializeMediaSource();
                }
            }
        }).catch(e => {
            console.error(`Error processing audio data: ${e.message}`);
            handleMediaError(e);
        });
    } else {
        // Process text data (likely track info)
        try {
            if (debugMode) console.log(`Received text message: ${event.data}`);
            const info = JSON.parse(event.data);
            
            // Check if this is an error message
            if (info.error) {
                console.error(`Server error: ${info.error}`);
                showStatus(`Server error: ${info.error}`, true);
                return;
            }
            
            // Check if track has changed
            const newTrackId = info.path;
            if (currentTrackId !== newTrackId) {
                console.log(`Track changed from ${currentTrackId} to ${newTrackId}`);
                currentTrackId = newTrackId;
                lastKnownPosition = 0;
                
                // Reset progress bar for new track
                updateProgressBar(0, info.duration);
            }
            
            // Update display
            currentTitle.textContent = info.title || 'Unknown Title';
            currentArtist.textContent = info.artist || 'Unknown Artist';
            currentAlbum.textContent = info.album || 'Unknown Album';
            
            // Update progress info
            if (info.duration) {
                currentDuration.textContent = formatTime(info.duration);
            }
            
            if (info.playback_position !== undefined) {
                currentPosition.textContent = formatTime(info.playback_position);
                updateProgressBar(info.playback_position, info.duration);
            }
            
            // Update listener count if available
            if (info.active_listeners !== undefined) {
                listenerCount.textContent = `Listeners: ${info.active_listeners}`;
            }
            
            // Store track ID
            currentTitle.dataset.trackId = info.path;
            
            // Update page title
            document.title = `${info.title} - ${info.artist} | Rust Web Radio`;
        } catch (e) {
            // If not valid JSON, just log the text message
            console.log(`Received non-JSON text message: ${event.data}`);
        }
    }
}

// Handle track transitions
function handleTrackTransition() {
    console.log('Handling track transition');
    
    // Reset progress
    lastKnownPosition = 0;
    updateProgressBar(0, 100); // Generic duration until we get real data
    
    // Clear audio queue
    audioQueue = [];
    isProcessingQueue = false;
    
    // Clear any pending audio data
    window.pendingAudioData = [];
    
    // If using MediaSource API, create a new MediaSource
    reinitializeMediaSource();
}

// Re-initialize MediaSource
function reinitializeMediaSource() {
    console.log('Reinitializing MediaSource');
    
    // Clean up existing MediaSource
    if (mediaSource) {
        if (mediaSource.readyState === 'open') {
            try {
                mediaSource.endOfStream();
            } catch (e) {
                console.log(`Error ending MediaSource: ${e.message}`);
            }
        }
        mediaSource = null;
    }
    
    // Clean up source buffer
    sourceBuffer = null;
    
    // Create new MediaSource
    try {
        mediaSource = new MediaSource();
        
        // Ensure audio element exists
        if (!audioElement) {
            audioElement = document.createElement('audio');
            audioElement.id = 'audio-stream';
            audioElement.controls = false;
            audioElement.volume = volumeControl.value;
            audioElement.muted = isMuted;
            document.body.appendChild(audioElement);
        }
        
        // Create object URL from media source
        const oldSrc = audioElement.src;
        if (oldSrc && oldSrc.startsWith('blob:')) {
            URL.revokeObjectURL(oldSrc);
        }
        
        const mediaSourceUrl = URL.createObjectURL(mediaSource);
        audioElement.src = mediaSourceUrl;
        
        // Setup MediaSource event handlers
        mediaSource.addEventListener('sourceopen', onSourceOpen);
        mediaSource.addEventListener('sourceended', () => console.log('MediaSource ended'));
        mediaSource.addEventListener('sourceclose', () => console.log('MediaSource closed'));
        mediaSource.addEventListener('error', (e) => console.error('MediaSource error:', e));
        
    } catch (e) {
        console.error(`Error reinitializing MediaSource: ${e.message}`);
        handleMediaError(e);
    }
}

// MediaSource open handler
function onSourceOpen() {
    console.log(`MediaSource opened, readyState: ${mediaSource.readyState}`);
    
    try {
        // Create source buffer for MP3
        const mimeType = 'audio/mpeg';
        if (!MediaSource.isTypeSupported(mimeType)) {
            throw new Error(`Unsupported MIME type: ${mimeType}`);
        }
        
        sourceBuffer = mediaSource.addSourceBuffer(mimeType);
        console.log('SourceBuffer created for audio/mpeg');
        
        // Setup source buffer event handlers
        sourceBuffer.addEventListener('updateend', function() {
            isProcessingQueue = false;
            processQueue();
        });
        
        sourceBuffer.addEventListener('error', (e) => {
            console.error('SourceBuffer error:', e);
        });
        
        // Set the mode to sequence if supported
        if ('mode' in sourceBuffer) {
            sourceBuffer.mode = 'sequence';
        }
        
        // Reset audio queue
        isProcessingQueue = false;
        
        // Process any pending audio data
        if (window.pendingAudioData && window.pendingAudioData.length > 0) {
            console.log(`Processing ${window.pendingAudioData.length} pending audio chunks`);
            audioQueue = window.pendingAudioData;
            window.pendingAudioData = [];
            processQueue();
        }
        
        // Start playback if not already playing
        if (audioElement.paused) {
            audioElement.play().catch(e => {
                console.error(`Error starting playback: ${e.message}`);
            });
        }
        
    } catch (e) {
        console.error(`Error setting up SourceBuffer: ${e.message}`);
        handleMediaError(e);
    }
}

// Handle media errors
function handleMediaError(error) {
    console.error(`Media error occurred: ${error.message || error}`);
    showStatus(`Audio error: ${error.name || 'Unknown error'}. Trying to recover...`, true);
    
    // Clean up completely
    if (sourceBuffer) {
        sourceBuffer = null;
    }
    
    if (mediaSource) {
        mediaSource = null;
    }
    
    if (audioElement) {
        audioElement.pause();
        audioElement.src = '';
        audioElement.load();
    }
    
    // Attempt reconnection with exponential backoff
    if (reconnectAttempts < maxReconnectAttempts) {
        reconnectAttempts++;
        const delay = Math.min(1000 * Math.pow(2, reconnectAttempts - 1), 10000);
        
        console.log(`Attempting reconnection ${reconnectAttempts}/${maxReconnectAttempts} in ${delay}ms`);
        showStatus(`Connection error. Reconnecting in ${delay/1000}s...`, true);
        
        setTimeout(() => {
            if (startBtn.dataset.connected === 'true') {
                startAudio();
            }
        }, delay);
    } else {
        showStatus('Unable to establish connection. Please try again later.', true);
        startBtn.textContent = 'Connect';
        startBtn.disabled = false;
        startBtn.dataset.connected = 'false';
    }
}

// Start audio and connection
function startAudio() {
    console.log('Starting audio playback');
    startBtn.disabled = true;
    
    // Reset reconnect attempts
    reconnectAttempts = 0;
    
    // Check if WebSocket API is supported
    if (!('WebSocket' in window)) {
        showStatus('Your browser does not support WebSockets. Please try a different browser.', true);
        startBtn.disabled = false;
        return;
    }
    
    // Check if MediaSource API is supported
    if (!('MediaSource' in window)) {
        showStatus('Your browser does not support MediaSource. Please try a different browser.', true);
        startBtn.disabled = false;
        return;
    }
    
    // Connect to WebSocket and setup audio
    connectWebSocket();
    
    // Start frequent checks of now playing info
    if (checkNowPlayingInterval) {
        clearInterval(checkNowPlayingInterval);
    }
    checkNowPlayingInterval = setInterval(updateNowPlaying, 2000);
}

// Establish WebSocket connection
function connectWebSocket() {
    // Clean up any existing WebSocket
    if (ws) {
        ws.close();
        ws = null;
    }
    
    showStatus('Connecting to stream...');
    
    try {
        // Create MediaSource
        reinitializeMediaSource();
        
        // Determine the WebSocket URL
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/stream`;
        console.log(`Connecting to WebSocket at ${wsUrl}`);
        
        // Create WebSocket connection
        ws = new WebSocket(wsUrl);
        
        ws.onopen = function() {
            console.log('WebSocket connection established');
            showStatus('Connected to audio stream');
            startBtn.textContent = 'Disconnect';
            startBtn.disabled = false;
            startBtn.dataset.connected = 'true';
            isPlaying = true;
            
            // Clear connection timeout if set
            if (connectionTimeout) {
                clearTimeout(connectionTimeout);
                connectionTimeout = null;
            }
            
            // Start playback
            if (audioElement) {
                audioElement.play().catch(function(e) {
                    console.error(`Error starting playback: ${e.message}`);
                    
                    if (e.name === 'NotAllowedError') {
                        showStatus('Click play to start audio (browser requires user interaction)', true);
                    } else {
                        showStatus('Error starting playback. Please try again.', true);
                    }
                    
                    // Enable the button so user can try again
                    startBtn.disabled = false;
                });
            }
        };
        
        ws.onclose = function(event) {
            console.log(`WebSocket connection closed: Code ${event.code}`);
            
            // Only attempt reconnect if it wasn't requested by the user
            if (startBtn.dataset.connected === 'true' && isPlaying) {
                handleStreamError('Connection lost. Attempting to reconnect...');
            } else {
                showStatus('Disconnected');
            }
        };
        
        ws.onerror = function(error) {
            console.error('WebSocket error occurred');
            handleStreamError('Error connecting to audio stream');
        };
        
        ws.onmessage = handleWebSocketMessage;
        
        // Set connection timeout
        connectionTimeout = setTimeout(function() {
            console.error('Connection timeout - no audio data received');
            handleStreamError('Connection timeout. Please try again.');
        }, 10000);
        
    } catch (e) {
        console.error(`WebSocket creation error: ${e.message}`);
        handleStreamError(`Failed to create WebSocket: ${e.message}`);
    }
}

// Handle stream errors
function handleStreamError(message) {
    console.error(`Stream error: ${message}`);
    showStatus(message, true);
    
    // Clean up
    stopAudio(true);
    
    // Try to reconnect if appropriate
    if (reconnectAttempts < maxReconnectAttempts) {
        reconnectAttempts++;
        const delay = Math.min(1000 * Math.pow(2, reconnectAttempts - 1), 10000);
        
        console.log(`Reconnect attempt ${reconnectAttempts} in ${delay}ms`);
        showStatus(`Connection lost. Reconnecting in ${delay/1000}s...`, true);
        
        setTimeout(() => {
            if (startBtn.dataset.connected === 'true') {
                startAudio();
            }
        }, delay);
    } else {
        console.log('Max reconnect attempts reached');
        showStatus('Could not connect to the server. Please try again later.', true);
        
        // Reset UI
        startBtn.textContent = 'Connect';
        startBtn.disabled = false;
        startBtn.dataset.connected = 'false';
    }
}

// Stop audio playback and disconnect
function stopAudio(isError = false) {
    console.log(`Stopping audio playback${isError ? ' due to error' : ' by user request'}`);
    
    isPlaying = false;
    
    // Clear any intervals
    if (checkNowPlayingInterval) {
        clearInterval(checkNowPlayingInterval);
        checkNowPlayingInterval = null;
    }
    
    // Clear connection timeout
    if (connectionTimeout) {
        clearTimeout(connectionTimeout);
        connectionTimeout = null;
    }
    
    // Close WebSocket if open
    if (ws) {
        ws.close();
        ws = null;
    }
    
    // Clean up MediaSource
    if (sourceBuffer) {
        sourceBuffer = null;
    }
    
    if (mediaSource) {
        if (mediaSource.readyState === 'open') {
            try {
                mediaSource.endOfStream();
            } catch (e) {
                console.log(`Error ending MediaSource: ${e.message}`);
            }
        }
        mediaSource = null;
    }
    
    // Clean up audio element
    if (audioElement) {
        audioElement.pause();
        audioElement.src = '';
        if (audioElement.parentNode) {
            audioElement.parentNode.removeChild(audioElement);
        }
        audioElement = null;
    }
    
    if (!isError) {
        showStatus('Disconnected from audio stream');
    }
    
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
            console.error(`Failed to fetch now playing info: ${response.status}`);
            return;
        }
        
        const data = await response.json();
        
        if (data.error) {
            currentTitle.textContent = 'No tracks available';
            currentArtist.textContent = 'Please add MP3 files to the server';
            currentAlbum.textContent = '';
            currentDuration.textContent = '';
            currentPosition.textContent = '';
            console.error(`Now playing error: ${data.error}`);
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
            currentPosition.textContent = formatTime(data.playback_position);
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

// Enhanced WebSocket message handling
function handleWebSocketMessage(event) {
    // Clear connection timeout if set
    if (connectionTimeout) {
        clearTimeout(connectionTimeout);
        connectionTimeout = null;
    }
    
    // Reset the audioLastUpdateTime
    audioLastUpdateTime = Date.now();
    
    // Process binary audio data
    if (event.data instanceof Blob) {
        // Log binary message size occasionally
        if (Math.random() < 0.005) { // Log roughly 0.5% of binary messages to reduce console spam
            logDebug(`Received binary data: ${event.data.size} bytes`, 'ws');
        }
        
        // Convert blob to array buffer
        event.data.arrayBuffer().then(buffer => {
            // Check for special markers
            if (buffer.byteLength === 2) {
                const view = new Uint8Array(buffer);
                
                // Track transition marker
                if (view[0] === 0xFF && view[1] === 0xFE) {
                    logDebug('Track transition detected - preparing for new track', 'ws');
                    
                    // Reset MediaSource for clean track transition
                    handleTrackTransition();
                    return;
                }
                
                // Track end marker
                if (view[0] === 0xFF && view[1] === 0xFF) {
                    logDebug('Track end marker received', 'ws');
                    return;
                }
            }
            
            // Handle empty buffer (track end or flush)
            if (buffer.byteLength === 0) {
                return;
            }
            
            // Process normal audio data
            if (sourceBuffer && mediaSource && mediaSource.readyState === 'open') {
                // Add to queue
                audioQueue.push(buffer);
                
                // Track maximum queue size for diagnostics
                if (audioQueue.length > maxQueueSize) {
                    maxQueueSize = audioQueue.length;
                    if (maxQueueSize % 10 === 0 && maxQueueSize > 30) {
                        logDebug(`Queue size peak: ${maxQueueSize} chunks`, 'audio');
                    }
                }
                
                // Process queue if not already processing
                if (!isProcessingQueue && !sourceBuffer.updating) {
                    processQueue();
                }
            } else {
                // If MediaSource isn't ready, we might need to recreate it
                if (!mediaSource || mediaSource.readyState !== 'open') {
                    logDebug('MediaSource not ready for audio data, attempting recovery', 'audio', true);
                    
                    // Store the audio data temporarily
                    if (!window.pendingAudioData) {
                        window.pendingAudioData = [];
                    }
                    window.pendingAudioData.push(buffer);
                    
                    // Try to reinitialize MediaSource
                    reinitializeMediaSource();
                }
            }
        }).catch(e => {
            logDebug(`Error processing audio data: ${e.message}`, 'audio', true);
            handleMediaError(e);
        });
    } else {
        // Process text data (likely track info)
        try {
            logDebug(`Received text message: ${event.data}`, 'ws');
            const info = JSON.parse(event.data);
            
            // Check if this is an error message
            if (info.error) {
                logDebug(`Server error: ${info.error}`, 'ws', true);
                showStatus(`Server error: ${info.error}`, true);
                return;
            }
            
            // Check if track has changed
            const newTrackId = info.path;
            if (currentTrackId !== newTrackId) {
                logDebug(`Track changed from ${currentTrackId} to ${newTrackId}`, 'track');
                currentTrackId = newTrackId;
                lastKnownPosition = 0;
                
                // Reset progress bar for new track
                updateProgressBar(0, info.duration);
                
                // Reset buffer statistics
                mediaBufferState.lastChunkCount = 0;
                mediaBufferState.bufferFullness = 0;
                mediaBufferState.underruns = 0;
                mediaBufferState.overruns = 0;
            }
            
            // Update display
            currentTitle.textContent = info.title || 'Unknown Title';
            currentArtist.textContent = info.artist || 'Unknown Artist';
            currentAlbum.textContent = info.album || 'Unknown Album';
            currentDuration.textContent = formatTime(info.duration);
            
            // Store track ID
            currentTitle.dataset.trackId = info.path;
            
            // Update page title
            document.title = `${info.title} - ${info.artist} | Rust Web Radio`;
        } catch (e) {
            // If not valid JSON, just log the text message
            logDebug(`Received non-JSON text message: ${event.data}`, 'ws');
        }
    }
}

// Improved track transition handling
function handleTrackTransition() {
    logDebug('Handling track transition - resetting audio system', 'audio');
    
    // Clean up queue but don't clear it completely
    // Keep any recently received chunks as they might be for the new track
    if (audioQueue.length > 10) {
        logDebug(`Trimming audio queue from ${audioQueue.length} to 10 chunks for transition`, 'audio');
        audioQueue = audioQueue.slice(-10);
    }
    
    isProcessingQueue = false;
    
    // Clear any pending audio data
    window.pendingAudioData = [];
    
    // Reset buffer statistics
    mediaBufferState.lastChunkCount = 0;
    mediaBufferState.bufferFullness = 0;
    mediaBufferState.underruns = 0;
    mediaBufferState.overruns = 0;
    
    // Check if MediaSource is still viable
    if (mediaSource && mediaSource.readyState === 'open' && sourceBuffer) {
        try {
            // Try to keep using the same MediaSource by just clearing the buffer
            if (sourceBuffer.updating) {
                sourceBuffer.abort();
                logDebug('Aborted current sourceBuffer operations', 'audio');
            }
            
            // Remove all buffered data
            if (sourceBuffer.buffered.length > 0) {
                const start = sourceBuffer.buffered.start(0);
                const end = sourceBuffer.buffered.end(sourceBuffer.buffered.length - 1);
                
                logDebug(`Clearing buffer data from ${start} to ${end} for track transition`, 'audio');
                
                sourceBuffer.remove(start, end);
                
                // Wait for buffer clear before proceeding
                sourceBuffer.addEventListener('updateend', function onBufferCleared() {
                    sourceBuffer.removeEventListener('updateend', onBufferCleared);
                    logDebug('Buffer cleared for track transition', 'audio');
                    // Start processing any queued data
                    isProcessingQueue = false;
                    if (audioQueue.length > 0) {
                        processQueue();
                    }
                }, { once: true });
                
                // Reset timers and counters
                lastKnownPosition = 0;
                return; // We'll continue after buffer is cleared
            }
        } catch (e) {
            logDebug(`Error during buffer clear: ${e.message}, will recreate MediaSource`, 'audio', true);
            // If buffer clear fails, fall back to recreating the MediaSource
        }
    }
    
    // If we got here, we need to recreate the MediaSource
    logDebug('Recreating MediaSource for track transition', 'audio');
    setTimeout(() => {
        reinitializeMediaSource();
    }, 100);
}

// Improved MediaSource initialization
function reinitializeMediaSource() {
    logDebug('Reinitializing MediaSource', 'audio');
    
    // Clean up existing MediaSource
    if (mediaSource) {
        if (mediaSource.readyState === 'open') {
            try {
                mediaSource.endOfStream();
                logDebug('Ended previous MediaSource stream', 'audio');
            } catch (e) {
                logDebug(`Error ending MediaSource: ${e.message}`, 'audio');
            }
        }
        
        // Proper cleanup to avoid memory leaks
        mediaSource.removeEventListener('sourceopen', onSourceOpen);
        mediaSource.removeEventListener('sourceended', onSourceEnded);
        mediaSource.removeEventListener('sourceclose', onSourceClose);
        mediaSource.removeEventListener('error', onMediaSourceError);
        mediaSource = null;
    }
    
    // Clean up source buffer
    if (sourceBuffer) {
        sourceBuffer.removeEventListener('updateend', processQueue);
        sourceBuffer.removeEventListener('error', handleSourceBufferError);
        sourceBuffer = null;
    }
    
    // Create new MediaSource
    try {
        mediaSource = new MediaSource();
        logDebug(`Created new MediaSource, readyState: ${mediaSource.readyState}`, 'audio');
        
        // Ensure audio element exists
        if (!audioElement) {
            audioElement = document.createElement('audio');
            audioElement.id = 'audio-stream';
            audioElement.controls = false;
            audioElement.volume = volumeControl.value;
            audioElement.muted = isMuted;
            document.body.appendChild(audioElement);
            
            // Set event listeners on new audio element
            audioElement.addEventListener('playing', () => {
                logDebug('Audio playback started', 'audio');
                showStatus('Audio playing');
            });
            
            audioElement.addEventListener('waiting', () => {
                logDebug('Audio buffering - waiting for more data', 'audio');
                showStatus('Buffering...');
            });
            
            audioElement.addEventListener('stalled', () => {
                logDebug('Audio playback stalled', 'audio', true);
                showStatus('Audio stalled - check connection', true);
            });
            
            audioElement.addEventListener('error', (e) => {
                const error = e.target.error;
                if (error) {
                    logDebug(`Audio error: ${error.message} (code: ${error.code})`, 'audio', true);
                    handleMediaError(error);
                } else {
                    logDebug('Unknown audio error', 'audio', true);
                    handleMediaError(new Error('Unknown audio error'));
                }
            });
        }
        
        // Create object URL from media source
        const oldSrc = audioElement.src;
        if (oldSrc && oldSrc.startsWith('blob:')) {
            URL.revokeObjectURL(oldSrc);
        }
        
        const mediaSourceUrl = URL.createObjectURL(mediaSource);
        audioElement.src = mediaSourceUrl;
        
        // Setup MediaSource event handlers
        mediaSource.addEventListener('sourceopen', onSourceOpen);
        mediaSource.addEventListener('sourceended', onSourceEnded);
        mediaSource.addEventListener('sourceclose', onSourceClose);
        mediaSource.addEventListener('error', onMediaSourceError);
        
    } catch (e) {
        logDebug(`Error reinitializing MediaSource: ${e.message}`, 'audio', true);
        handleMediaError(e);
    }
}

// Improved source open handler
function onSourceOpen() {
    logDebug(`MediaSource opened, readyState: ${mediaSource.readyState}`, 'audio');
    
    try {
        // Create source buffer for MP3
        const mimeType = 'audio/mpeg';
        if (!MediaSource.isTypeSupported(mimeType)) {
            throw new Error(`Unsupported MIME type: ${mimeType}`);
        }
        
        sourceBuffer = mediaSource.addSourceBuffer(mimeType);
        logDebug('SourceBuffer created for audio/mpeg', 'audio');
        
        // Set mode - 'segments' mode works better for audio streaming
        if ('mode' in sourceBuffer) {
            sourceBuffer.mode = 'segments';
            logDebug('SourceBuffer mode set to segments for better audio streaming', 'audio');
        }
        
        // Setup source buffer event handlers
        sourceBuffer.addEventListener('updateend', processQueue);
        sourceBuffer.addEventListener('error', handleSourceBufferError);
        
        sourceBuffer.addEventListener('abort', () => {
            logDebug('SourceBuffer abort event', 'audio');
        });
        
        // Reset processing state
        isProcessingQueue = false;
        
        // Process any pending audio data
        if (window.pendingAudioData && window.pendingAudioData.length > 0) {
            logDebug(`Processing ${window.pendingAudioData.length} pending audio chunks`, 'audio');
            
            // Add all pending chunks to the queue
            audioQueue = [...window.pendingAudioData, ...audioQueue];
            window.pendingAudioData = [];
            
            // Start processing
            processQueue();
        }
        
        // Start playback if not already playing
        if (audioElement.paused) {
            audioElement.play().catch(e => {
                logDebug(`Error starting playback: ${e.message}`, 'audio');
                
                if (e.name === 'NotAllowedError') {
                    showStatus('Click play to start audio (browser requires user interaction)', true);
                }
            });
        }
        
    } catch (e) {
        logDebug(`Error setting up SourceBuffer: ${e.message}`, 'audio', true);
        handleMediaError(e);
    }
}

// Initialize with diagnostic information
function initialize() {
    logDebug('Initializing web radio player', 'general');
    
    // Add diagnostic info to status
    const audioContextSupport = ('AudioContext' in window) || ('webkitAudioContext' in window);
    const mediaSourceSupport = 'MediaSource' in window;
    const mp3Support = mediaSourceSupport && MediaSource.isTypeSupported('audio/mpeg');
    
    logDebug(`Browser support: AudioContext=${audioContextSupport}, MediaSource=${mediaSourceSupport}, MP3=${mp3Support}`, 'general');
    
    if (!mp3Support) {
        showStatus('Warning: Your browser may not fully support MP3 streaming. Try Chrome or Firefox for best results.', true);
    }
    
    // Check for Worklet support (modern audio processing)
    const workletSupport = audioContextSupport && 'audioWorklet' in AudioContext.prototype;
    logDebug(`Advanced audio features: AudioWorklet=${workletSupport}`, 'general');
    
    // Set up event listeners
    setupEventListeners();
    
    // Set initial button state
    startBtn.textContent = 'Connect';
    startBtn.dataset.connected = 'false';
    
    // Set initial volume
    const savedVolume = localStorage.getItem('radioVolume');
    if (savedVolume !== null) {
        volumeControl.value = savedVolume;
    }
    
    // Add statistics display if in debug mode
    if (DEBUG) {
        createDebugPanel();
    }
    
    // Update now playing display
    updateNowPlaying();
    
    // Start background updates
    setInterval(updateStats, 10000);
    setInterval(checkConnectionHealth, 5000);
    
    logDebug('Initialization complete - waiting for user to click Connect', 'general');
}

// Check connection health periodically
function checkConnectionHealth() {
    if (!isPlaying || !ws || !audioElement) return;
    
    const now = Date.now();
    const timeSinceLastUpdate = now - audioLastUpdateTime;
    
    // Check if we've received audio data recently
    if (timeSinceLastUpdate > 10000) { // 10 seconds without data
        logDebug(`No audio data received for ${timeSinceLastUpdate}ms`, 'ws', true);
        
        if (ws.readyState === WebSocket.OPEN) {
            // Connection appears open but no data - try to ping
            try {
                ws.send('ping');
                logDebug('Sent ping to check connection', 'ws');
            } catch (e) {
                logDebug(`Error sending ping: ${e.message}`, 'ws', true);
            }
            
            // Set a timeout to check if we get a response
            setTimeout(() => {
                if (Date.now() - audioLastUpdateTime > 15000) { // Still no data after 15 seconds
                    logDebug('Connection appears dead despite being open, forcing reconnect', 'ws', true);
                    handleStreamError('Connection stalled. Reconnecting...');
                }
            }, 5000);
        } else {
            // Connection is not open
            logDebug(`WebSocket state: ${ws.readyState}`, 'ws', true);
            handleStreamError('Connection lost. Reconnecting...');
        }
    }
    
    // Check audio element state
    if (audioElement && audioElement.paused && isPlaying) {
        logDebug('Audio element is paused but should be playing - attempting to resume', 'audio', true);
        
        audioElement.play().catch(e => {
            logDebug(`Error resuming playback: ${e.message}`, 'audio', true);
        });
    }
    
    // Check buffer health
    updateBufferStats();
}

// Create a debug panel for monitoring
function createDebugPanel() {
    // Only create if it doesn't exist
    if (document.getElementById('debug-panel')) return;
    
    // Create container
    debugContainer = document.createElement('div');
    debugContainer.id = 'debug-panel';
    debugContainer.style.cssText = 'position:fixed; bottom:10px; right:10px; width:400px; height:200px; background:rgba(0,0,0,0.8); color:#0f0; font-family:monospace; font-size:10px; padding:10px; border-radius:5px; z-index:1000; overflow:auto;';
    
    // Create header
    const header = document.createElement('div');
    header.textContent = 'Debug Log';
    header.style.cssText = 'border-bottom:1px solid #0f0; margin-bottom:5px; padding-bottom:5px;';
    
    // Create log container
    debugLog = document.createElement('div');
    debugLog.id = 'debug-log';
    
    // Add controls
    const controls = document.createElement('div');
    controls.style.cssText = 'display:flex; justify-content:space-between; margin-top:5px;';
    
    const clearBtn = document.createElement('button');
    clearBtn.textContent = 'Clear';
    clearBtn.style.cssText = 'background:#333; color:#fff; border:none; padding:2px 5px; cursor:pointer;';
    clearBtn.onclick = () => {
        debugLog.innerHTML = '';
    };
    
    const hideBtn = document.createElement('button');
    hideBtn.textContent = 'Hide';
    hideBtn.style.cssText = 'background:#333; color:#fff; border:none; padding:2px 5px; cursor:pointer;';
    hideBtn.onclick = () => {
        debugContainer.style.display = 'none';
    };
    
    controls.appendChild(clearBtn);
    controls.appendChild(hideBtn);
    
    // Assemble
    debugContainer.appendChild(header);
    debugContainer.appendChild(debugLog);
    debugContainer.appendChild(controls);
    
    // Add to body
    document.body.appendChild(debugContainer);
    
    // Log initial debug message
    logDebug('Debug panel created', 'general');
}

// Initialize when the DOM is loaded
document.addEventListener('DOMContentLoaded', initialize);