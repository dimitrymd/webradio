// Enhanced player.js with better error handling and recovery
// This fixes the MediaSource handling issues that can cause playback to fail

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
const currentDuration = document.getElementById('current-duration');
const currentPosition = document.getElementById('current-position');
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
let isProcessingQueue = false;

// Track current state for better recovery
let currentTrackId = null;
let lastKnownPosition = 0;

// Debug configuration
const DEBUG = true;
const DEBUG_AUDIO = true;
const DEBUG_WEBSOCKET = true;
const DEBUG_TRACK_INFO = true;

// Debug UI elements
let debugContainer = null;
let debugLog = null;

// Enhanced debug logging function
function logDebug(message, type = 'general', isError = false) {
    if (!DEBUG) return;
    
    // Skip certain high-volume logging if specific debug flags are off
    if (type === 'audio' && !DEBUG_AUDIO) return;
    if (type === 'ws' && !DEBUG_WEBSOCKET) return;
    if (type === 'track' && !DEBUG_TRACK_INFO) return;
    
    const timestamp = new Date().toISOString().substring(11, 23);
    
    // Log to browser console
    if (isError) {
        console.error(`[${timestamp}] [${type}] ${message}`);
    } else {
        console.log(`[${timestamp}] [${type}] ${message}`);
    }
    
    // Log to debug UI if it exists
    if (debugLog) {
        const entry = document.createElement('div');
        entry.className = `debug-entry type-${type}${isError ? ' error' : ''}`;
        entry.innerHTML = `<span class="timestamp">[${timestamp}]</span> ${message}`;
        
        debugLog.insertBefore(entry, debugLog.firstChild);
        
        // Limit entries to prevent browser slowdown
        if (debugLog.children.length > 1000) {
            debugLog.removeChild(debugLog.lastChild);
        }
    }
}

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
    }
}

// Show status message
function showStatus(message, isError = false) {
    logDebug(`Status message: ${message}${isError ? ' (ERROR)' : ''}`, 'general', isError);
    
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

// Enhanced error recovery function
function handleMediaError(error) {
    logDebug(`Media error occurred: ${error.message || error}`, 'audio', true);
    
    // Clean up completely
    cleanupMediaSource();
    
    // Handle different types of errors
    if (error.name === 'NotSupportedError') {
        showStatus('Your browser does not support streaming audio. Please try a different browser.', true);
        startBtn.disabled = false;
        return;
    }
    
    if (error.name === 'QuotaExceededError') {
        logDebug('Media buffer quota exceeded, attempting recovery', 'audio', true);
        // This error requires special handling - we need to clear buffers
        if (sourceBuffer && mediaSource && mediaSource.readyState === 'open' && sourceBuffer.buffered.length > 0) {
            try {
                const start = sourceBuffer.buffered.start(0);
                const end = sourceBuffer.buffered.end(sourceBuffer.buffered.length - 1);
                sourceBuffer.remove(start, end);
                logDebug(`Cleared buffer from ${start} to ${end}`, 'audio');
            } catch (removeError) {
                logDebug(`Failed to clear buffer: ${removeError.message}`, 'audio', true);
            }
        }
    }
    
    // Attempt reconnection with exponential backoff
    if (reconnectAttempts < maxReconnectAttempts) {
        reconnectAttempts++;
        const delay = Math.min(1000 * Math.pow(2, reconnectAttempts - 1), 10000);
        
        logDebug(`Attempting reconnection ${reconnectAttempts}/${maxReconnectAttempts} in ${delay}ms`, 'audio');
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

// Enhanced cleanup function
function cleanupMediaSource() {
    logDebug('Cleaning up media source', 'audio');
    
    // Stop any ongoing processing
    isProcessingQueue = false;
    audioQueue = [];
    
    // Clean up MediaSource and SourceBuffer
    if (sourceBuffer) {
        try {
            // Remove all event listeners
            sourceBuffer.removeEventListener('updateend', processQueue);
            sourceBuffer.removeEventListener('error', handleSourceBufferError);
            
            // Abort any pending operations
            if (sourceBuffer.updating) {
                sourceBuffer.abort();
            }
        } catch (e) {
            logDebug(`Error cleaning up source buffer: ${e.message}`, 'audio', true);
        }
        sourceBuffer = null;
    }
    
    if (mediaSource) {
        try {
            if (mediaSource.readyState === 'open') {
                mediaSource.endOfStream();
            }
        } catch (e) {
            logDebug(`Error ending media source: ${e.message}`, 'audio', true);
        }
        mediaSource = null;
    }
    
    // Clean up audio element
    if (audioElement) {
        audioElement.pause();
        audioElement.removeAttribute('src');
        if (audioElement.parentNode) {
            audioElement.parentNode.removeChild(audioElement);
        }
        audioElement = null;
    }
}

// Enhanced source buffer error handler
function handleSourceBufferError(e) {
    logDebug(`SourceBuffer error: ${e.message || 'Unknown error'}`, 'audio', true);
    handleMediaError(new Error('SourceBuffer error'));
}

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
        if (Math.random() < 0.01) { // Log roughly 1% of binary messages
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
                    
                    // CRITICAL: Reset MediaSource for clean track transition
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
                logDebug('Empty buffer received (track end or flush)', 'ws');
                return;
            }
            
            // Process normal audio data
            if (sourceBuffer && mediaSource && mediaSource.readyState === 'open') {
                // Add to queue
                audioQueue.push(buffer);
                
                // Process queue if not already processing
                if (!isProcessingQueue && !sourceBuffer.updating) {
                    processQueue();
                }
                
                // If queue is getting too large, log a warning
                if (audioQueue.length > 100) {
                    logDebug(`Warning: Audio queue growing large: ${audioQueue.length} chunks`, 'audio');
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
            logDebug(`Parsed track info: ${JSON.stringify(info)}`, 'track');
            
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
                
                // Reinitialize audio system for new track
                reinitializeMediaSource();
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

// Handle track transitions properly
function handleTrackTransition() {
    logDebug('Handling track transition - resetting audio system', 'audio');
    
    // Clear audio queue
    audioQueue = [];
    isProcessingQueue = false;
    
    // Clear any pending audio data
    window.pendingAudioData = [];
    
    // Reset MediaSource and SourceBuffer
    if (sourceBuffer && mediaSource && mediaSource.readyState === 'open') {
        try {
            // Abort any pending operations
            if (sourceBuffer.updating) {
                sourceBuffer.abort();
            }
            
            // Remove all buffered data
            if (sourceBuffer.buffered.length > 0) {
                const start = sourceBuffer.buffered.start(0);
                const end = sourceBuffer.buffered.end(sourceBuffer.buffered.length - 1);
                sourceBuffer.remove(start, end);
                logDebug(`Cleared buffer data from ${start} to ${end} for track transition`, 'audio');
            }
        } catch (e) {
            logDebug(`Error during track transition cleanup: ${e.message}`, 'audio', true);
        }
    }
    
    // Reset timers and counters
    lastKnownPosition = 0;
    
    // Reinitialize the MediaSource for the new track
    setTimeout(() => {
        reinitializeMediaSource();
    }, 100);
}

// Reinitialize MediaSource for new track or after error
function reinitializeMediaSource() {
    logDebug('Reinitializing MediaSource', 'audio');
    
    // Clean up existing MediaSource
    if (mediaSource) {
        if (mediaSource.readyState === 'open') {
            try {
                mediaSource.endOfStream();
            } catch (e) {
                logDebug(`Error ending MediaSource: ${e.message}`, 'audio');
            }
        }
        mediaSource = null;
    }
    
    // Clean up source buffer
    if (sourceBuffer) {
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
        
        // Clear audio queue
        audioQueue = [];
        isProcessingQueue = false;
        
    } catch (e) {
        logDebug(`Error reinitializing MediaSource: ${e.message}`, 'audio', true);
        handleMediaError(e);
    }
}

// Update the onSourceOpen handler to process any pending audio data
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
        
        // Setup source buffer event handlers
        sourceBuffer.addEventListener('updateend', function() {
            processQueue();
        });
        
        sourceBuffer.addEventListener('error', handleSourceBufferError);
        
        sourceBuffer.addEventListener('abort', () => {
            logDebug('SourceBuffer abort event', 'audio');
        });
        
        // Set the mode to sequence if supported
        if ('mode' in sourceBuffer) {
            sourceBuffer.mode = 'sequence';
            logDebug('SourceBuffer mode set to sequence', 'audio');
        }
        
        // Reset audio queue
        audioQueue = [];
        isProcessingQueue = false;
        
        // Process any pending audio data
        if (window.pendingAudioData && window.pendingAudioData.length > 0) {
            logDebug(`Processing ${window.pendingAudioData.length} pending audio chunks`, 'audio');
            audioQueue = window.pendingAudioData;
            window.pendingAudioData = [];
            processQueue();
        }
        
        // Start playback if not already playing
        if (audioElement.paused) {
            audioElement.play().catch(e => {
                logDebug(`Error starting playback: ${e.message}`, 'audio');
            });
        }
        
    } catch (e) {
        logDebug(`Error setting up SourceBuffer: ${e.message}`, 'audio', true);
        handleMediaError(e);
    }
}

// Enhanced queue processing with better error recovery
function processQueue() {
    if (audioQueue.length > 0 && !isProcessingQueue && sourceBuffer && !sourceBuffer.updating) {
        isProcessingQueue = true;
        const data = audioQueue.shift();
        
        try {
            // Check buffer size before appending
            if (sourceBuffer.buffered.length > 0) {
                const bufferedEnd = sourceBuffer.buffered.end(sourceBuffer.buffered.length - 1);
                const bufferedStart = sourceBuffer.buffered.start(0);
                const bufferedDuration = bufferedEnd - bufferedStart;
                
                // If we have too much buffered, remove old data
                if (bufferedDuration > 30) { // Keep max 30 seconds
                    const removeEnd = bufferedStart + 10; // Remove first 10 seconds
                    sourceBuffer.remove(bufferedStart, removeEnd);
                    logDebug(`Removed old buffer data from ${bufferedStart} to ${removeEnd}`, 'audio');
                    
                    // Wait for remove to complete
                    sourceBuffer.addEventListener('updateend', function onRemoveEnd() {
                        sourceBuffer.removeEventListener('updateend', onRemoveEnd);
                        // Now try to append the data
                        try {
                            sourceBuffer.appendBuffer(data);
                        } catch (e) {
                            handleAppendError(e, data);
                        }
                    });
                    
                    isProcessingQueue = false;
                    return;
                }
            }
            
            // Normal append
            sourceBuffer.appendBuffer(data);
            
            // Log queue status periodically
            if (audioQueue.length % 50 === 0 && audioQueue.length > 0) {
                logDebug(`Queue status: ${audioQueue.length} chunks pending`, 'audio');
            }
        } catch (e) {
            handleAppendError(e, data);
        }
    } else {
        // All other conditions lead to resetting the processing flag
        isProcessingQueue = false;
    }
}

// Add helper function for handling append errors
function handleAppendError(e, data) {
    logDebug(`Error appending buffer: ${e.name} - ${e.message}`, 'audio', true);
    
    if (e.name === 'QuotaExceededError') {
        // Buffer is full, try to clear some space
        if (sourceBuffer.buffered.length > 0) {
            const bufferedStart = sourceBuffer.buffered.start(0);
            const currentTime = audioElement.currentTime;
            const removeEnd = Math.min(currentTime - 5, bufferedStart + 10);
            
            if (removeEnd > bufferedStart) {
                try {
                    sourceBuffer.remove(bufferedStart, removeEnd);
                    // Put the data back in queue
                    audioQueue.unshift(data);
                } catch (removeError) {
                    logDebug(`Error removing buffer: ${removeError.message}`, 'audio', true);
                }
            }
        }
    }
    
    isProcessingQueue = false;
}

// Enhanced connection start with better MediaSource handling
function startAudio() {
    logDebug('Starting audio playback - user initiated', 'audio');
    startBtn.disabled = true;
    
    // Reset reconnect attempts
    reconnectAttempts = 0;
    
    // Check if WebSocket API is supported
    if (!('WebSocket' in window)) {
        logDebug('WebSocket API not supported by this browser', 'audio', true);
        showStatus('Your browser does not support WebSockets. Please try a different browser.', true);
        startBtn.disabled = false;
        return;
    }
    
    // Check if MediaSource API is supported
    if (!('MediaSource' in window) || !MediaSource.isTypeSupported('audio/mpeg')) {
        logDebug('MediaSource API not supported for MP3 by this browser', 'audio', true);
        showStatus('Your browser does not fully support MediaSource for MP3. Audio may not play correctly.', true);
        // We'll still try to connect, but warn the user
    }
    
    logDebug('Connecting to WebSocket stream...', 'audio');
    connectWebSocket();
    
    // Start frequent checks of now playing info
    if (checkNowPlayingInterval) {
        clearInterval(checkNowPlayingInterval);
    }
    checkNowPlayingInterval = setInterval(updateNowPlaying, 2000);
}

// Enhanced WebSocket connection with better error handling
function connectWebSocket() {
    // Clean up any existing WebSocket
    if (ws) {
        logDebug('Closing existing WebSocket connection', 'ws');
        ws.close();
        ws = null;
    }
    
    // Clean up any existing MediaSource
    cleanupMediaSource();
    
    try {
        // Create MediaSource first
        mediaSource = new MediaSource();
        logDebug(`Created MediaSource object, readyState: ${mediaSource.readyState}`, 'audio');
        
        // Create audio element and attach MediaSource
        audioElement = document.createElement('audio');
        audioElement.id = 'audio-stream';
        audioElement.controls = false; // Hide controls
        
        // Set initial properties
        audioElement.volume = volumeControl.value;
        audioElement.muted = isMuted;
        
        // Add to DOM (required for some browsers)
        document.body.appendChild(audioElement);
        
        // Create object URL from media source
        const mediaSourceUrl = URL.createObjectURL(mediaSource);
        audioElement.src = mediaSourceUrl;
        
        // Setup MediaSource event handlers
        mediaSource.addEventListener('sourceopen', onSourceOpen);
        mediaSource.addEventListener('sourceended', onSourceEnded);
        mediaSource.addEventListener('sourceclose', onSourceClose);
        mediaSource.addEventListener('error', onMediaSourceError);
        
        // Setup audio element event handlers
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
        
        audioElement.addEventListener('ended', () => {
            logDebug('Audio ended', 'audio');
        });
        
    } catch (e) {
        logDebug(`Error setting up media: ${e.message}`, 'audio', true);
        handleMediaError(e);
    }
}

// Enhanced MediaSource open handler
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
        
        // Setup source buffer event handlers
        sourceBuffer.addEventListener('updateend', function() {
            // Process the queue when source buffer is ready
            processQueue();
        });
        
        sourceBuffer.addEventListener('error', handleSourceBufferError);
        
        sourceBuffer.addEventListener('abort', () => {
            logDebug('SourceBuffer abort event', 'audio');
        });
        
        // Set the mode to sequence if supported
        if ('mode' in sourceBuffer) {
            sourceBuffer.mode = 'sequence';
            logDebug('SourceBuffer mode set to sequence', 'audio');
        }
        
        // Reset audio queue
        audioQueue = [];
        isProcessingQueue = false;
        
        // Now setup WebSocket connection
        setupWebSocket();
        
    } catch (e) {
        logDebug(`Error setting up SourceBuffer: ${e.message}`, 'audio', true);
        handleMediaError(e);
    }
}

// WebSocket setup as separate function
function setupWebSocket() {
    // Determine the WebSocket URL
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/stream`;
    logDebug(`Connecting to WebSocket at ${wsUrl}`, 'ws');
    
    try {
        // Create WebSocket connection
        ws = new WebSocket(wsUrl);
        
        ws.onopen = function() {
            logDebug('WebSocket connection established', 'ws');
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
            audioElement.play().catch(function(e) {
                logDebug(`Error starting playback: ${e.message}`, 'audio', true);
                // Don't immediately show error - might need user interaction
                if (e.name === 'NotAllowedError') {
                    showStatus('Click play to start audio (browser requires user interaction)', true);
                } else {
                    showStatus('Error starting playback. Please try again.', true);
                }
                
                // Enable the button so user can try again
                startBtn.disabled = false;
            });
        };
        
        ws.onclose = function(event) {
            logDebug(`WebSocket connection closed: Code ${event.code}, Reason: ${event.reason}`, 'ws');
            
            // Only attempt reconnect if it wasn't requested by the user
            if (startBtn.dataset.connected === 'true' && isPlaying) {
                handleStreamError('Connection lost. Attempting to reconnect...');
            } else {
                showStatus('Disconnected');
            }
        };
        
        ws.onerror = function(error) {
            logDebug('WebSocket error occurred', 'ws', true);
            handleStreamError('Error connecting to audio stream');
        };
        
        ws.onmessage = handleWebSocketMessage;
        
        // Set connection timeout
        connectionTimeout = setTimeout(function() {
            logDebug('Connection timeout - no audio data received', 'audio', true);
            handleStreamError('Connection timeout. Please try again.');
        }, 10000);
        
    } catch (e) {
        logDebug(`WebSocket creation error: ${e.message}`, 'ws', true);
        handleStreamError(`Failed to create WebSocket: ${e.message}`);
    }
}

// MediaSource event handlers
function onSourceEnded() {
    logDebug('MediaSource ended', 'audio');
}

function onSourceClose() {
    logDebug('MediaSource closed', 'audio');
}

function onMediaSourceError(e) {
    logDebug(`MediaSource error: ${e.message || 'Unknown error'}`, 'audio', true);
    handleMediaError(new Error('MediaSource error'));
}

// Enhanced stream error handler
function handleStreamError(message) {
    logDebug(`Stream error: ${message}`, 'audio', true);
    showStatus(message, true);
    
    // Clean up
    stopAudio(true);
    
    // Try to reconnect if appropriate
    if (reconnectAttempts < maxReconnectAttempts) {
        reconnectAttempts++;
        const delay = Math.min(1000 * Math.pow(2, reconnectAttempts - 1), 10000); // Exponential backoff
        
        logDebug(`Reconnect attempt ${reconnectAttempts} in ${delay}ms`, 'audio');
        showStatus(`Connection lost. Reconnecting in ${delay/1000}s...`, true);
        
        setTimeout(() => {
            if (startBtn.dataset.connected === 'true') {
                logDebug('Attempting to reconnect...', 'audio');
                startAudio();
            }
        }, delay);
    } else {
        logDebug('Max reconnect attempts reached', 'audio', true);
        showStatus('Could not connect to the server. Please try again later.', true);
        
        // Reset UI
        startBtn.textContent = 'Connect';
        startBtn.disabled = false;
        startBtn.dataset.connected = 'false';
    }
}

// Enhanced stop function
function stopAudio(isError = false) {
    logDebug(`Stopping audio playback${isError ? ' due to error' : ' by user request'}`, 'audio');
    
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
    
    // Clean up media source
    cleanupMediaSource();
    
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
        logDebug('User requested disconnect', 'general');
        stopAudio();
    } else {
        logDebug('User requested connect - starting audio now', 'general');
        startAudio();
    }
}

// Enhanced now playing update function
async function updateNowPlaying() {
    try {
        logDebug("Fetching now playing info...", 'track');
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            logDebug(`Failed to fetch now playing info: ${response.status}`, 'track', true);
            throw new Error(`Failed to fetch now playing info: ${response.status}`);
        }
        
        const data = await response.json();
        logDebug(`Received now playing info: ${JSON.stringify(data)}`, 'track');
        
        if (data.error) {
            currentTitle.textContent = 'No tracks available';
            currentArtist.textContent = 'Please add MP3 files to the server';
            currentAlbum.textContent = '';
            currentDuration.textContent = '';
            currentPosition.textContent = '';
            logDebug(`Now playing error: ${data.error}`, 'track', true);
        } else {
            // Store track ID (path) for change detection
            const newTrackId = data.path;
            const trackChanged = currentTitle.dataset.trackId !== newTrackId;
            
            if (trackChanged) {
                logDebug(`Track changed to: "${data.title}" by "${data.artist}"`, 'track');
                
                // Update track ID
                currentTitle.dataset.trackId = newTrackId;
                currentTrackId = newTrackId;
                
                // Reset position tracking
                lastKnownPosition = 0;
                
                // Clear progress bar
                updateProgressBar(0, data.duration);
            }
            
            currentTitle.textContent = data.title || 'Unknown Title';
            currentArtist.textContent = data.artist || 'Unknown Artist';
            currentAlbum.textContent = data.album || 'Unknown Album';
            currentDuration.textContent = formatTime(data.duration);
            
            // Update position if available
            if (data.playback_position !== undefined) {
                currentPosition.textContent = formatTime(data.playback_position);
                updateProgressBar(data.playback_position, data.duration);
                lastKnownPosition = data.playback_position;
                logDebug(`Playback position: ${data.playback_position}s / ${data.duration}s`, 'track');
            }
            
            // Update listener count if available
            if (data.active_listeners !== undefined) {
                listenerCount.textContent = `Listeners: ${data.active_listeners}`;
                logDebug(`Active listeners: ${data.active_listeners}`, 'track');
            }
            
            // Update page title
            document.title = `${data.title} - ${data.artist} | Rust Web Radio`;
            
            // Update the last update time
            audioLastUpdateTime = Date.now();
        }
    } catch (error) {
        logDebug(`Error fetching now playing: ${error.message}`, 'track', true);
        // Don't show error to user - this is a background update
    }
}

// Update stats
async function updateStats() {
    try {
        const response = await fetch('/api/stats');
        const data = await response.json();
        
        // Update listener count
        listenerCount.textContent = `Listeners: ${data.active_listeners}`;
        logDebug(`Updated stats: ${JSON.stringify(data)}`, 'general');
    } catch (error) {
        logDebug(`Error fetching stats: ${error.message}`, 'general', true);
    }
}

// Event listener setup
function setupEventListeners() {
    startBtn.addEventListener('click', toggleConnection);
    
    volumeControl.addEventListener('input', () => {
        if (audioElement) {
            audioElement.volume = volumeControl.value;
            logDebug(`Volume set to ${volumeControl.value}`, 'audio');
        }
        
        localStorage.setItem('radioVolume', volumeControl.value);
    });
    
    muteBtn.addEventListener('click', () => {
        if (audioElement) {
            audioElement.muted = !audioElement.muted;
            muteBtn.textContent = audioElement.muted ? 'Unmute' : 'Mute';
            isMuted = audioElement.muted;
            logDebug(`Audio ${audioElement.muted ? 'muted' : 'unmuted'}`, 'audio');
        }
    });
    
    // Handle page visibility changes
    document.addEventListener('visibilitychange', () => {
        if (document.visibilityState === 'visible') {
            logDebug('Page is now visible', 'general');
            
            // Update now playing
            updateNowPlaying();
            
            // Reconnect if needed and if the user was previously connected
            if (startBtn.dataset.connected === 'true' && (!ws || ws.readyState !== WebSocket.OPEN)) {
                logDebug('Reconnecting after page became visible', 'audio');
                // Add a short delay to allow the browser to stabilize after becoming visible
                setTimeout(() => {
                    startAudio();
                }, 500);
            }
        } else {
            logDebug('Page is now hidden', 'general');
        }
    });
    
    // Handle page reload/unload
    window.addEventListener('beforeunload', () => {
        // Properly clean up resources
        logDebug('Page unloading, cleaning up resources', 'general');
        
        if (ws) {
            ws.close();
        }
        
        cleanupMediaSource();
    });
    
    // Handle online/offline events
    window.addEventListener('online', () => {
        logDebug('Browser is back online', 'general');
        if (startBtn.dataset.connected === 'true' && (!ws || ws.readyState !== WebSocket.OPEN)) {
            setTimeout(() => {
                startAudio();
            }, 1000);
        }
    });
    
    window.addEventListener('offline', () => {
        logDebug('Browser is offline', 'general');
        showStatus('No internet connection', true);
    });
}

// Initialize the application
function initialize() {
    logDebug('Initializing web radio player', 'general');
    
    // Check browser compatibility
    checkBrowserSupport();
    
    // Set up event listeners
    setupEventListeners();
    
    // Set initial button state
    startBtn.textContent = 'Connect';
    startBtn.dataset.connected = 'false';
    
    // Set initial volume
    const savedVolume = localStorage.getItem('radioVolume');
    if (savedVolume !== null) {
        volumeControl.value = savedVolume;
        logDebug(`Restored saved volume: ${savedVolume}`, 'audio');
    }
    
    // Update now playing display
    updateNowPlaying();
    
    // Regular stats update
    setInterval(updateStats, 10000);
    
    logDebug('Initialization complete - waiting for user to click Connect', 'general');
}

// Browser compatibility check
function checkBrowserSupport() {
    const issues = [];
    
    if (!('MediaSource' in window)) {
        issues.push('MediaSource API not supported');
    } else if (!MediaSource.isTypeSupported('audio/mpeg')) {
        issues.push('MP3 streaming not supported in MediaSource');
    }
    
    if (!('WebSocket' in window)) {
        issues.push('WebSocket API not supported');
    }
    
    if (!('AudioContext' in window) && !('webkitAudioContext' in window)) {
        issues.push('AudioContext API not supported');
    }
    
    if (issues.length > 0) {
        logDebug(`Browser compatibility issues: ${issues.join(', ')}`, 'general', true);
        showStatus('Your browser may not fully support this application. Please use a modern browser.', true);
    } else {
        logDebug('Browser compatibility check passed', 'general');
    }
}

// Start the application when DOM is ready
document.addEventListener('DOMContentLoaded', initialize);