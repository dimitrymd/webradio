// unified-player.js - Progressive fallback audio player for all browsers including iOS
// This replaces your existing player.js, player-audio.js, and player-connection.js

// Player state and configuration
const state = {
    // Playback method
    playbackMethod: null, // Will be set to 'mse', 'webaudio', or 'direct'
    
    // Audio elements and contexts
    audioElement: null,     // Used for all methods
    mediaSource: null,      // Used for MSE method
    sourceBuffer: null,     // Used for MSE method
    audioContext: null,     // Used for WebAudio method
    audioSourceNode: null,  // Used for WebAudio method
    audioQueue: [],         // Used for MSE and WebAudio methods
    
    // Connection and status
    ws: null,
    isPlaying: false,
    isMuted: false,
    volume: 0.7,
    connectionTimeout: null,
    reconnectAttempts: 0,
    maxReconnectAttempts: 15,
    lastAudioChunkTime: Date.now(),
    lastTrackInfoTime: Date.now(),
    
    // Track info
    currentTrackId: null,
    lastKnownPosition: 0,
    
    // Timers and monitoring
    connectionHealthTimer: null,
    nowPlayingTimer: null,
    lastErrorTime: 0,
    consecutiveErrors: 0,
};

// Configuration constants
const config = {
    // ⚠️ INCREASED BUFFER SIZES
    TARGET_BUFFER_SIZE: 20,         // Increased from 10 to 20 seconds
    MIN_BUFFER_SIZE: 5,             // Increased from 3 to 5 seconds
    MAX_BUFFER_SIZE: 60,            // Increased from 30 to 60 seconds
    BUFFER_MONITOR_INTERVAL: 2000,  // More frequent checking (from 3000 to 2000ms)
    
    // ⚠️ IMPROVED TIMEOUT VALUES
    NO_DATA_TIMEOUT: 30,            // Increased from 20 to 30 seconds
    AUDIO_STARVATION_THRESHOLD: 3,  // Increased from 2 to 3 seconds
    
    NOW_PLAYING_INTERVAL: 10000,    // Keep this the same
    SHOW_DEBUG_INFO: true,          // Enable to show more info during debugging
    
    // ⚠️ NEW BUFFER PARAMETERS
    CHUNK_BATCH_SIZE: 5,            // Process chunks in batches for efficiency
    BUFFER_CHECK_FREQUENCY: 0.2,    // Check buffer health on 20% of timeupdate events
    INITIAL_BUFFER_CHUNKS: 50,      // Wait for more chunks before starting playback
};

// UI Elements
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

// When we detect iOS or other Apple products
const isAppleDevice = /iPad|iPhone|iPod|Mac/.test(navigator.userAgent) && !window.MSStream;

// Initialize player with best available method
function initPlayer() {
    log("Initializing radio player...");
    
    // Set up event listeners
    startBtn.addEventListener('click', toggleConnection);
    
    muteBtn.addEventListener('click', function() {
        state.isMuted = !state.isMuted;
        
        if (state.audioElement) {
            state.audioElement.muted = state.isMuted;
        }
        
        muteBtn.textContent = state.isMuted ? 'Unmute' : 'Mute';
    });
    
    volumeControl.addEventListener('input', function() {
        state.volume = this.value;
        
        if (state.audioElement) {
            state.audioElement.volume = state.volume;
        }
        
        try {
            localStorage.setItem('radioVolume', state.volume);
        } catch (e) {
            // Ignore storage errors
        }
    });
    
    // Load saved volume from localStorage
    try {
        const savedVolume = localStorage.getItem('radioVolume');
        if (savedVolume !== null) {
            volumeControl.value = savedVolume;
            state.volume = parseFloat(savedVolume);
        }
    } catch (e) {
        // Ignore storage errors
    }
    
    // Fetch initial track info
    fetchNowPlaying();
    
    determinePlaybackMethod();
    
    log('ChillOut Radio player initialized');
}

// Determine best playback method based on browser capabilities
function determinePlaybackMethod() {
    // Default method selection strategy
    let methodToUse = 'direct'; // Fallback is always direct streaming
    
    if ('MediaSource' in window && MediaSource.isTypeSupported('audio/mpeg')) {
        methodToUse = 'mse';
        log('Using MediaSource Extensions (MSE) for playback');
    } else if ('AudioContext' in window || 'webkitAudioContext' in window) {
        methodToUse = 'webaudio';
        log('Using WebAudio API for playback');
    } else {
        log('Using direct HTTP streaming for playback');
    }
    
    // Override for Apple devices to use direct streaming
    if (isAppleDevice) {
        methodToUse = 'direct';
        log('Apple device detected. Forcing direct streaming method.');
    }
    
    state.playbackMethod = methodToUse;
    
    // Show in UI if debug is enabled
    if (config.SHOW_DEBUG_INFO) {
        showStatus(`Playback method: ${methodToUse.toUpperCase()}`, false, false);
    }
    
    return methodToUse;
}

// Start audio playback using the determined method
function startAudio() {
    log(`Starting audio playback using ${state.playbackMethod} method`, 'CONTROL');
    startBtn.disabled = true;
    showStatus('Connecting to stream...', false, false);
    
    // Reset state
    state.reconnectAttempts = 0;
    state.audioQueue = [];
    state.consecutiveErrors = 0;
    state.lastAudioChunkTime = Date.now();
    state.lastTrackInfoTime = Date.now();
    state.isPlaying = true;
    
    // Create audio element if needed
    if (!state.audioElement) {
        state.audioElement = new Audio();
        state.audioElement.controls = false;
        state.audioElement.volume = state.volume;
        state.audioElement.muted = state.isMuted;
        state.audioElement.preload = 'auto';
        // Add to document but hide visually
        state.audioElement.style.display = 'none';
        document.body.appendChild(state.audioElement);
        
        // Set up common audio event listeners
        setupCommonAudioListeners();
    }
    
    // Use appropriate method
    switch (state.playbackMethod) {
        case 'mse':
            startMSEPlayback();
            break;
        case 'webaudio':
            startWebAudioPlayback();
            break;
        case 'direct':
        default:
            startDirectPlayback();
            break;
    }
    
    // Set up now playing update timer
    if (state.nowPlayingTimer) {
        clearInterval(state.nowPlayingTimer);
    }
    state.nowPlayingTimer = setInterval(fetchNowPlaying, config.NOW_PLAYING_INTERVAL);
    
    // Start connection health check timer
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
    }
    state.connectionHealthTimer = setInterval(checkConnectionHealth, config.BUFFER_MONITOR_INTERVAL);
}

// Stop audio playback and disconnect
function stopAudio(isError = false) {
    log(`Stopping audio playback${isError ? ' (due to error)' : ''}`, 'CONTROL');
    
    state.isPlaying = false;
    
    // Clear all timers
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
        state.connectionHealthTimer = null;
    }
    
    if (state.nowPlayingTimer) {
        clearInterval(state.nowPlayingTimer);
        state.nowPlayingTimer = null;
    }
    
    if (state.connectionTimeout) {
        clearTimeout(state.connectionTimeout);
        state.connectionTimeout = null;
    }
    
    // Close WebSocket if we're using it
    if (state.ws) {
        try {
            state.ws.close();
        } catch (e) {
            // Ignore close errors
        }
        state.ws = null;
    }
    
    // Method-specific cleanup
    switch (state.playbackMethod) {
        case 'mse':
            stopMSEPlayback();
            break;
        case 'webaudio':
            stopWebAudioPlayback();
            break;
        default:
            // For direct, just stop the audio element
            break;
    }
    
    // Stop audio
    if (state.audioElement) {
        state.audioElement.pause();
        state.audioElement.src = '';
        state.audioElement.load();
    }
    
    // Clear queue
    state.audioQueue = [];
    
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
        log('User requested disconnect', 'CONTROL');
        stopAudio();
    } else {
        log('User requested connect', 'CONTROL');
        startAudio();
    }
}

// Update track info from WebSocket or API
function updateTrackInfo(info) {
    try {
        // Check for error message
        if (info.error) {
            showStatus(`Server error: ${info.error}`, true);
            return;
        }
        
        // Store track ID for change detection
        const newTrackId = info.path;
        if (state.currentTrackId !== newTrackId) {
            log(`Track changed to: ${info.title}`, 'TRACK');
            state.currentTrackId = newTrackId;
            
            // Reset position tracking
            state.lastKnownPosition = 0;
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
            state.lastKnownPosition = info.playback_position;
            updateProgressBar(info.playback_position, info.duration);
        }
        
        // Update listener count
        if (info.active_listeners !== undefined) {
            listenerCount.textContent = `Listeners: ${info.active_listeners}`;
        }
        
        // Store track ID in DOM for future comparison
        currentTitle.dataset.trackId = info.path;
        
        // Update page title
        document.title = `${info.title} - ${info.artist} | ChillOut Radio`;
        
        // Update last track info time
        state.lastTrackInfoTime = Date.now();
    } catch (e) {
        log(`Error processing track info: ${e.message}`, 'TRACK', true);
    }
}

// Set up common audio event listeners
function setupCommonAudioListeners() {
    state.audioElement.addEventListener('playing', () => {
        log('Audio playing', 'AUDIO');
        showStatus('Audio playing');
    });
    
    state.audioElement.addEventListener('waiting', () => {
        log('Audio buffering', 'AUDIO');
        showStatus('Buffering...', false, false);
    });
    
    state.audioElement.addEventListener('stalled', () => {
        log('Audio stalled', 'AUDIO');
        showStatus('Stream stalled - buffering', true, false);
    });
    
    state.audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        log(`Audio error (code ${errorCode})`, 'AUDIO', true);
        
        // Only react to errors if we're still trying to play
        if (state.isPlaying) {
            // Don't react to errors too frequently
            const now = Date.now();
            if (now - state.lastErrorTime > 10000) { // At most one error response per 10 seconds
                state.lastErrorTime = now;
                showStatus('Audio error - attempting to recover', true, false);
                
                // Try reconnecting
                attemptReconnection();
            }
        }
    });
    
    state.audioElement.addEventListener('ended', () => {
        log('Audio ended', 'AUDIO');
        // If we shouldn't be at the end, try to restart
        if (state.isPlaying) {
            log('Audio ended unexpectedly, attempting to recover', 'AUDIO', true);
            showStatus('Audio ended - reconnecting', true, false);
            attemptReconnection();
        }
    });
}

//
// MSE Method Implementation
//

// ⚠️ IMPROVED: Start MSE playback with better buffering
function startMSEPlayback() {
    // Reset state for playback
    state.audioQueue = [];
    state.initialBuffering = true;  // Start with initialBuffering flag set
    
    // Set up MediaSource with error handling
    try {
        // Create MediaSource
        state.mediaSource = new MediaSource();
        
        // Set up event handlers
        state.mediaSource.addEventListener('sourceopen', () => {
            log('MediaSource opened', 'MEDIA');
            
            try {
                // Create source buffer for MP3
                state.sourceBuffer = state.mediaSource.addSourceBuffer('audio/mpeg');
                
                // Add buffer monitoring event
                state.sourceBuffer.addEventListener('updateend', () => {
                    // Process queue when update ends
                    processQueue();
                    
                    // Also periodically check buffer health and trim if needed
                    if (Math.random() < 0.1) { // Only do this check occasionally
                        const bufferHealth = getBufferHealth();
                        
                        // If buffer is getting very large, trim it
                        if (bufferHealth.duration > config.MAX_BUFFER_SIZE) {
                            const currentTime = state.audioElement.currentTime;
                            const trimPoint = Math.max(state.sourceBuffer.buffered.start(0), currentTime - 20);
                            log(`Trimming buffer: ${trimPoint.toFixed(2)}s to current time - 20`, 'BUFFER');
                            try {
                                state.sourceBuffer.remove(state.sourceBuffer.buffered.start(0), trimPoint);
                            } catch (e) {
                                log(`Error trimming buffer: ${e.message}`, 'BUFFER');
                            }
                        }
                    }
                });
                
                // Add enhanced audio listeners
                setupEnhancedAudioListeners();
                
                // Connect to WebSocket after MediaSource is ready
                connectWebSocket();
                
                // Show buffering message initially
                showStatus('Buffering stream...', false, false);
            } catch (e) {
                log(`Error creating source buffer: ${e.message}`, 'MEDIA', true);
                showStatus(`Media error: ${e.message}`, true);
                startBtn.disabled = false;
            }
        });
        
        state.mediaSource.addEventListener('sourceended', () => log('MediaSource ended', 'MEDIA'));
        state.mediaSource.addEventListener('sourceclose', () => log('MediaSource closed', 'MEDIA'));
        
        // Create object URL and set as audio source
        const url = URL.createObjectURL(state.mediaSource);
        state.audioElement.src = url;
        
        // Do NOT start playing here, wait for buffer to fill
        log('MSE playback initialized, waiting for buffer to fill', 'MEDIA');
        
    } catch (e) {
        log(`MediaSource setup error: ${e.message}`, 'MEDIA', true);
        showStatus(`Media error: ${e.message}`, true);
        startBtn.disabled = false;
    }
}

function setupEnhancedAudioListeners() {
    // Add buffer monitoring to timeupdate event
    state.audioElement.addEventListener('timeupdate', () => {
        // Only check occasionally to reduce overhead
        if (Math.random() < config.BUFFER_CHECK_FREQUENCY) {
            const bufferHealth = getBufferHealth();
            
            // If buffer is critically low but we have data in queue, process immediately
            if (bufferHealth.ahead < config.AUDIO_STARVATION_THRESHOLD && state.audioQueue.length > 0) {
                if (!state.sourceBuffer.updating) {
                    log(`Buffer critically low (${bufferHealth.ahead.toFixed(2)}s), processing queue`, 'BUFFER');
                    processQueue();
                }
            }
            
            // Show buffering UI if we're low on buffer but not during initial buffering
            if (bufferHealth.ahead < 0.5 && !state.initialBuffering) {
                showStatus('Buffering...', false, false);
            }
        }
    });
    
    // Enhanced stalled handler
    state.audioElement.addEventListener('stalled', () => {
        log('Audio stalled - possible network interruption', 'AUDIO', true);
        showStatus('Stream stalled - buffering', true, false);
        
        // If we have data in queue but audio stalled, we might need to recreate MediaSource
        if (state.audioQueue.length > 10) {
            log('Audio stalled with data in queue, recreating MediaSource', 'AUDIO', true);
            recreateMediaSource();
        }
    });
}

// ⚠️ NEW: Get current buffer health metrics
function getBufferHealth() {
    if (!state.sourceBuffer || !state.audioElement || state.playbackMethod !== 'mse' || 
        !state.sourceBuffer.buffered || state.sourceBuffer.buffered.length === 0) {
        return {
            current: 0,
            ahead: 0, 
            duration: 0,
            underflow: true
        };
    }
    
    const currentTime = state.audioElement.currentTime;
    const bufferedEnd = state.sourceBuffer.buffered.end(state.sourceBuffer.buffered.length - 1);
    const bufferAhead = bufferedEnd - currentTime;
    const totalBuffered = state.sourceBuffer.buffered.end(state.sourceBuffer.buffered.length - 1) - 
                         state.sourceBuffer.buffered.start(0);
    
    return {
        current: currentTime,
        ahead: bufferAhead,
        duration: totalBuffered,
        underflow: bufferAhead < config.AUDIO_STARVATION_THRESHOLD
    };
}


function stopMSEPlayback() {
    // Clean up MediaSource
    if (state.sourceBuffer) {
        state.sourceBuffer = null;
    }
    
    if (state.mediaSource && state.mediaSource.readyState === 'open') {
        try {
            state.mediaSource.endOfStream();
        } catch (e) {
            log(`Error ending MediaSource: ${e.message}`, 'MEDIA');
        }
    }
    state.mediaSource = null;
}

// Process audio data queue for MSE method
function processQueue() {
    // Exit conditions - ensure all necessary components are ready
    if (state.playbackMethod !== 'mse' || 
        state.audioQueue.length === 0 || 
        !state.sourceBuffer || 
        !state.mediaSource || 
        state.mediaSource.readyState !== 'open' || 
        state.sourceBuffer.updating) {
        return;
    }
    
    // ⚠️ NEW: Check buffer health to make decisions
    const bufferHealth = getBufferHealth();
    const queueSizeInChunks = state.audioQueue.length;
    
    // ⚠️ NEW: Log buffer status more frequently during buffering
    if (bufferHealth.ahead < config.AUDIO_STARVATION_THRESHOLD * 2) {
        log(`Buffer health critical: ${bufferHealth.ahead.toFixed(1)}s ahead, queue: ${queueSizeInChunks} chunks`, 'BUFFER');
    }
    
    // ⚠️ NEW: Handle initial buffering differently
    if (state.initialBuffering) {
        if (queueSizeInChunks >= config.INITIAL_BUFFER_CHUNKS || bufferHealth.ahead > config.MIN_BUFFER_SIZE * 1.5) {
            log(`Initial buffer filled: ${bufferHealth.ahead.toFixed(1)}s ahead, queue: ${queueSizeInChunks} chunks`, 'BUFFER');
            state.initialBuffering = false;
            
            // Start playback if paused
            if (state.audioElement && state.audioElement.paused && state.isPlaying) {
                state.audioElement.play().catch(e => {
                    log(`Error playing after buffer filled: ${e.message}`, 'AUDIO', true);
                });
            }
        } else {
            // Wait for more data during initial buffering
            setTimeout(processQueue, 100);
            return;
        }
    }
    
    try {
        // ⚠️ NEW: Process chunks in batches for better throughput
        let processedCount = 0;
        let batchSize = Math.min(config.CHUNK_BATCH_SIZE, state.audioQueue.length);
        
        // If buffer is critically low, process more chunks at once
        if (bufferHealth.ahead < config.AUDIO_STARVATION_THRESHOLD) {
            batchSize = Math.min(10, state.audioQueue.length); // Process more chunks
        }
        
        // Process first chunk immediately
        const data = state.audioQueue.shift();
        state.sourceBuffer.appendBuffer(data);
        state.lastAudioChunkTime = Date.now();
        processedCount++;
        
        // ⚠️ NEW: Set up callback to process more chunks when this append completes
        state.sourceBuffer.addEventListener('updateend', function onUpdateEnd() {
            state.sourceBuffer.removeEventListener('updateend', onUpdateEnd);
            
            // Process more chunks in this batch if available
            if (processedCount < batchSize && state.audioQueue.length > 0 && state.sourceBuffer && !state.sourceBuffer.updating) {
                try {
                    const nextData = state.audioQueue.shift();
                    state.sourceBuffer.appendBuffer(nextData);
                    processedCount++;
                    
                    // Continue with this event handler for the batch
                    state.sourceBuffer.addEventListener('updateend', onUpdateEnd);
                } catch (e) {
                    log(`Error processing batch item ${processedCount}: ${e.message}`, 'BUFFER', true);
                    // If we hit an error, process the rest with normal scheduling
                    processQueue();
                }
            } else {
                // Schedule processing of remaining queue after a short delay
                const delay = bufferHealth.ahead < config.MIN_BUFFER_SIZE ? 0 : 10;
                setTimeout(processQueue, delay);
            }
        });
        
        // Reset consecutive errors since we successfully processed data
        state.consecutiveErrors = 0;
        
    } catch (e) {
        log(`Error processing audio data: ${e.message}`, 'BUFFER', true);
        state.consecutiveErrors++;
        
        // Handle quota exceeded errors
        if (e.name === 'QuotaExceededError') {
            // ⚠️ IMPROVED: More aggressive buffer clearing for quota errors
            handleQuotaExceededError(bufferHealth);
        } else {
            // For other errors, try again soon with backoff
            const retryDelay = Math.min(50 * state.consecutiveErrors, 1000);
            setTimeout(processQueue, retryDelay);
        }
    }
}

// Handle quota exceeded errors for MSE
function handleQuotaExceededError(bufferHealth) {
    try {
        if (state.sourceBuffer && state.sourceBuffer.buffered.length > 0) {
            const currentTime = state.audioElement.currentTime;
            
            // Remove more aggressively when we hit quota errors
            const safeRemovalPoint = Math.max(
                state.sourceBuffer.buffered.start(0),
                currentTime - 1  // Keep only 1 second before current position
            );
            
            // Calculate how much we need to remove (more than before)
            const removalEnd = Math.min(
                safeRemovalPoint + 10,  // Remove 10 seconds of audio (increased from 5)
                currentTime - 0.5  // But never too close to current playback position
            );
            
            if (removalEnd > safeRemovalPoint) {
                log(`Clearing buffer segment ${safeRemovalPoint.toFixed(1)}-${removalEnd.toFixed(1)}s`, 'BUFFER');
                state.sourceBuffer.remove(safeRemovalPoint, removalEnd);
                
                // Continue after buffer clear
                state.sourceBuffer.addEventListener('updateend', function onClearEnd() {
                    state.sourceBuffer.removeEventListener('updateend', onClearEnd);
                    setTimeout(processQueue, 10);  // Reduced from 50ms to 10ms
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
        const savedQueue = state.audioQueue.slice(-50); // Keep only the last 50 chunks
        state.audioQueue = []; // Clear the queue
        
        // Clean up old MediaSource
        if (state.sourceBuffer) {
            state.sourceBuffer = null;
        }
        
        if (state.mediaSource && state.mediaSource.readyState === 'open') {
            try {
                state.mediaSource.endOfStream();
            } catch (e) {
                // Ignore errors during cleanup
            }
        }
        
        // Create new MediaSource
        state.mediaSource = new MediaSource();
        
        state.mediaSource.addEventListener('sourceopen', function onSourceOpen() {
            log('New MediaSource opened', 'MEDIA');
            
            try {
                // Create source buffer
                state.sourceBuffer = state.mediaSource.addSourceBuffer('audio/mpeg');
                
                // Setup error handler
                state.sourceBuffer.addEventListener('error', (event) => {
                    log(`SourceBuffer error: ${event.message || 'Unknown error'}`, 'MEDIA', true);
                });
                
                // Restore queue and continue
                state.audioQueue = savedQueue;
                state.consecutiveErrors = 0;
                setTimeout(processQueue, 100);
            } catch (e) {
                log(`Error creating source buffer: ${e.message}`, 'MEDIA', true);
                attemptReconnection();
            }
        });
        
        // Connect to audio element
        const url = URL.createObjectURL(state.mediaSource);
        state.audioElement.src = url;
        
        // Make sure we're playing
        if (state.audioElement.paused && state.isPlaying) {
            state.audioElement.play().catch(e => {
                log(`Error playing after recreation: ${e.message}`, 'MEDIA', true);
            });
        }
    } catch (e) {
        log(`Error recreating MediaSource: ${e.message}`, 'MEDIA', true);
        // If recreation fails, attempt reconnection as a last resort
        attemptReconnection();
    }
}

//
// WebAudio Method Implementation
//

function startWebAudioPlayback() {
    try {
        // Create AudioContext
        const AudioContext = window.AudioContext || window.webkitAudioContext;
        state.audioContext = new AudioContext();
        
        // Connect to WebSocket for data
        connectWebSocket();
        
        log('WebAudio playback initialized', 'AUDIO');
    } catch (e) {
        log(`WebAudio setup error: ${e.message}`, 'AUDIO', true);
        showStatus(`Audio initialization error: ${e.message}`, true);
        
        // Fall back to direct streaming
        log('Falling back to direct streaming method', 'AUDIO');
        state.playbackMethod = 'direct';
        startDirectPlayback();
    }
}

function stopWebAudioPlayback() {
    // Clean up WebAudio resources
    if (state.audioSourceNode) {
        try {
            state.audioSourceNode.disconnect();
        } catch (e) {
            // Ignore disconnection errors
        }
        state.audioSourceNode = null;
    }
    
    if (state.audioContext) {
        try {
            // Modern browsers support closing
            if (state.audioContext.state !== 'closed' && state.audioContext.close) {
                state.audioContext.close();
            }
        } catch (e) {
            // Ignore context close errors
        }
        state.audioContext = null;
    }
}

// Process audio data for WebAudio method
function processWebAudioData(audioData) {
    if (state.playbackMethod !== 'webaudio' || !state.audioContext) {
        return;
    }
    
    try {
        // Decode the audio data
        state.audioContext.decodeAudioData(audioData, 
            (buffer) => {
                playDecodedAudio(buffer);
            },
            (error) => {
                log(`Error decoding audio data: ${error}`, 'AUDIO', true);
            }
        );
    } catch (e) {
        log(`WebAudio processing error: ${e.message}`, 'AUDIO', true);
    }
}

// Play decoded audio buffer
function playDecodedAudio(audioBuffer) {
    if (!state.audioContext || !state.isPlaying) return;
    
    try {
        // Create source node
        const source = state.audioContext.createBufferSource();
        source.buffer = audioBuffer;
        
        // Connect to destination with volume control
        const gainNode = state.audioContext.createGain();
        gainNode.gain.value = state.volume;
        
        source.connect(gainNode);
        gainNode.connect(state.audioContext.destination);
        
        // Store current source node for later cleanup
        if (state.audioSourceNode) {
            try {
                state.audioSourceNode.disconnect();
            } catch (e) {
                // Ignore disconnect errors
            }
        }
        state.audioSourceNode = source;
        
        // Start playback
        source.start(0);
        
        // Set up ended handler to play next buffer
        source.onended = () => {
            if (state.audioQueue.length > 0 && state.isPlaying) {
                const nextData = state.audioQueue.shift();
                processWebAudioData(nextData);
            }
        };
    } catch (e) {
        log(`Error playing decoded audio: ${e.message}`, 'AUDIO', true);
    }
}

//
// Direct HTTP Streaming Method
//

function startDirectPlayback() {
    try {
        // Set up audio element for direct streaming
        const timestamp = Date.now(); // Prevent caching
        state.audioElement.src = `/direct-stream?t=${timestamp}`;
        
        // Set up event listeners specific to direct streaming
        state.audioElement.addEventListener('canplay', () => {
            log('Direct stream ready to play', 'AUDIO');
            showStatus('Stream ready');
            startBtn.textContent = 'Disconnect';
            startBtn.disabled = false;
            startBtn.dataset.connected = 'true';
        });
        
        // Start playing
        const playPromise = state.audioElement.play();
        playPromise.then(() => {
            log('Direct stream playback started', 'AUDIO');
            showStatus('Connected to stream');
        }).catch(e => {
            log(`Direct stream playback error: ${e.message}`, 'AUDIO', true);
            if (e.name === 'NotAllowedError') {
                showStatus('Click play to start audio (browser requires user interaction)', true, false);
                startBtn.disabled = false;
            } else {
                showStatus(`Playback error: ${e.message}`, true);
                startBtn.disabled = false;
            }
        });
        
        // Start periodic now playing updates
        fetchNowPlaying();
    } catch (e) {
        log(`Direct streaming error: ${e.message}`, 'AUDIO', true);
        showStatus(`Streaming error: ${e.message}`, true);
        stopAudio(true);
    }
}

//
// WebSocket Connection and Data Handling
//

function connectWebSocket() {
    // Don't connect WebSocket for direct streaming
    if (state.playbackMethod === 'direct') {
        return;
    }
    
    // Clean up any existing connection
    if (state.ws) {
        try {
            state.ws.close();
        } catch (e) {
            // Ignore close errors
        }
        state.ws = null;
    }
    
    try {
        // Determine WebSocket URL
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/stream`;
        log(`Connecting to WebSocket: ${wsUrl}`, 'STREAM');
        
        // Create connection
        state.ws = new WebSocket(wsUrl);
        
        // Set up event handlers
        state.ws.onopen = () => {
            log('WebSocket connection established', 'STREAM');
            showStatus('Connected to stream');
            startBtn.textContent = 'Disconnect';
            startBtn.disabled = false;
            startBtn.dataset.connected = 'true';
            
            // Reset reconnect attempts on successful connection
            state.reconnectAttempts = 0;
            
            // Request initial track info
            try {
                state.ws.send(JSON.stringify({ type: 'now_playing_request' }));
                log('Requested initial track info', 'TRACK');
            } catch (e) {
                log(`Error requesting track info: ${e.message}`, 'TRACK', true);
                // Fallback to API
                fetchNowPlaying();
            }
            
            // Start audio playback if using MSE
            if (state.playbackMethod === 'mse' && state.audioElement && state.audioElement.paused) {
                const playPromise = state.audioElement.play();
                playPromise.catch(e => {
                    log(`Play error: ${e.message}`, 'AUDIO', true);
                    if (e.name === 'NotAllowedError') {
                        showStatus('Click play to start audio (browser requires user interaction)', true, false);
                    }
                });
            }
        };
        
        state.ws.onclose = (event) => {
            log(`WebSocket closed: Code ${event.code}`, 'STREAM');
            
            // Only attempt reconnect if we're still supposed to be playing
            if (state.isPlaying) {
                // Use a brief delay to avoid hammering the server
                setTimeout(() => {
                    if (state.isPlaying) {
                        showStatus('Connection closed. Reconnecting...', true, false);
                        attemptReconnection();
                    }
                }, 1000);
            }
        };
        
        state.ws.onerror = (error) => {
            log('WebSocket error', 'STREAM', true);
            
            // Don't immediately try to reconnect - wait for the close event
            showStatus('Connection error', true, false);
        };
        
        state.ws.onmessage = handleWebSocketMessage;
        
        // Set connection timeout (increased for slower connections)
        state.connectionTimeout = setTimeout(() => {
            if (state.ws && state.audioQueue.length === 0) {
                log('Connection timeout - no data received', 'STREAM', true);
                showStatus('Connection timeout. Reconnecting...', true, false);
                attemptReconnection();
            }
        }, 20000);
        
    } catch (e) {
        log(`WebSocket setup error: ${e.message}`, 'STREAM', true);
        showStatus(`Connection error: ${e.message}`, true);
        attemptReconnection();
    }
}

// Handle WebSocket messages
function handleWebSocketMessage(event) {
    // Clear connection timeout if set
    if (state.connectionTimeout) {
        clearTimeout(state.connectionTimeout);
        state.connectionTimeout = null;
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
            
            // Add data to queue and process according to method
            state.lastAudioChunkTime = Date.now();
            
            // ⚠️ IMPROVED: Check queue size and warn if it's growing too large
            if (state.audioQueue.length > 500) {
                log(`Warning: Large queue size (${state.audioQueue.length} chunks)`, 'STREAM');
            }
            
            if (state.playbackMethod === 'mse') {
                state.audioQueue.push(buffer);
                
                // If source buffer is ready and not updating, process queue
                if (state.sourceBuffer && !state.sourceBuffer.updating) {
                    processQueue();
                }
                
                // Update UI during initial buffering
                if (state.initialBuffering && state.audioQueue.length % 10 === 0) {
                    showStatus(`Buffering: ${state.audioQueue.length}/${config.INITIAL_BUFFER_CHUNKS} chunks...`, false, false);
                }
                
            } else if (state.playbackMethod === 'webaudio') {
                if (state.audioContext && state.audioSourceNode && 
                    state.audioSourceNode.onended !== null) {
                    // Already playing a buffer, queue this one
                    state.audioQueue.push(buffer);
                } else {
                    // Start playing immediately
                    processWebAudioData(buffer);
                }
            }
            
        }).catch(e => {
            log(`Error processing binary data: ${e.message}`, 'STREAM', true);
        });
    } else {
        // Process text data (track info or other commands)
        try {
            const data = JSON.parse(event.data);
            
            // Check message type
            if (data.type === 'now_playing') {
                updateTrackInfo(data.track);
            } else {
                // Default treatment as track info
                updateTrackInfo(data);
            }
        } catch (e) {
            log(`Non-JSON message: ${event.data}`, 'STREAM');
        }
    }
}

//
// Connection Health Monitoring
//

// Check connection health
function checkConnectionHealth() {
    if (!state.isPlaying) return;
    
    const now = Date.now();
    const timeSinceLastAudio = (now - state.lastAudioChunkTime) / 1000;
    const timeSinceLastTrackInfo = (now - state.lastTrackInfoTime) / 1000;
    
    // For direct streaming, just check if it's still playing
    if (state.playbackMethod === 'direct') {
        if (state.audioElement && state.audioElement.paused && state.isPlaying) {
            log('Direct stream paused unexpectedly', 'HEALTH', true);
            showStatus('Stream interrupted. Reconnecting...', true, false);
            attemptReconnection();
        }
        
        // Check if we need to update now playing
        if (timeSinceLastTrackInfo > config.NOW_PLAYING_INTERVAL / 1000) {
            fetchNowPlaying();
        }
        
        return;
    }
    
    // For MSE and WebAudio, check WebSocket connection
    if (timeSinceLastAudio > config.NO_DATA_TIMEOUT) {
        log(`No audio data received for ${timeSinceLastAudio.toFixed(1)}s`, 'HEALTH', true);
        
        // Check if WebSocket is still connected
        if (state.ws && state.ws.readyState === WebSocket.OPEN) {
            try {
                state.ws.send(JSON.stringify({ type: 'ping' }));
                log('Sent ping to check connection', 'HEALTH');
            } catch (e) {
                log(`Error sending ping: ${e.message}`, 'HEALTH', true);
                attemptReconnection();
            }
        } else {
            // WebSocket disconnected
            showStatus('Connection lost. Reconnecting...', true, false);
            attemptReconnection();
        }
    }
    
    // Check if we need to request now playing info
    if (timeSinceLastTrackInfo > config.NOW_PLAYING_INTERVAL / 1000) {
        // Request now playing update via WebSocket
        if (state.ws && state.ws.readyState === WebSocket.OPEN) {
            try {
                state.ws.send(JSON.stringify({ type: 'now_playing_request' }));
                log('Requested now playing info via WebSocket', 'TRACK');
            } catch (e) {
                log(`Error requesting now playing: ${e.message}`, 'TRACK', true);
                // Fallback to API
                fetchNowPlaying();
            }
        } else {
            // Fallback to API if WebSocket not available
            fetchNowPlaying();
        }
    }
}

// Attempt reconnection with exponential backoff
function attemptReconnection() {
    // Don't try to reconnect if we're not supposed to be playing
    if (!state.isPlaying) return;
    
    // Check if we've reached the maximum attempts
    if (state.reconnectAttempts >= state.maxReconnectAttempts) {
        log(`Maximum reconnection attempts (${state.maxReconnectAttempts}) reached`, 'CONTROL', true);
        showStatus('Could not reconnect to server. Please try again later.', true);
        
        // Reset UI
        stopAudio(true);
        return;
    }
    
    // Increment attempts
    state.reconnectAttempts++;
    
    // Calculate delay with exponential backoff and a bit of randomness
    const baseDelay = Math.min(500 * Math.pow(1.3, state.reconnectAttempts - 1), 8000);
    const jitter = Math.random() * 1000; // Add up to 1 second of jitter
    const delay = baseDelay + jitter;
    
    log(`Reconnection attempt ${state.reconnectAttempts}/${state.maxReconnectAttempts} in ${(delay/1000).toFixed(1)}s`, 'CONTROL');
    showStatus(`Reconnecting (${state.reconnectAttempts}/${state.maxReconnectAttempts})...`, true, false);
    
    // Clean up existing connections
    if (state.ws) {
        try {
            state.ws.close();
        } catch (e) {
            // Ignore close errors
        }
        state.ws = null;
    }
    
    // Schedule reconnection
    setTimeout(() => {
        if (state.isPlaying) {
            // If we're having trouble, consider changing playback method as fallback
            if (state.reconnectAttempts > 3 && state.playbackMethod !== 'direct') {
                log(`Falling back to direct streaming after ${state.reconnectAttempts} failed attempts`, 'CONTROL');
                state.playbackMethod = 'direct';
                showStatus('Switching to direct streaming mode...', false, false);
            }
            
            // Clear all old state
            if (state.playbackMethod === 'mse') {
                stopMSEPlayback();
            } else if (state.playbackMethod === 'webaudio') {
                stopWebAudioPlayback();
            }
            
            // Restart with current method
            startAudio();
        }
    }, delay);
}

//
// API and Data Fetching
//

// Fetch now playing info via API
async function fetchNowPlaying() {
    try {
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            log(`Now playing API error: ${response.status}`, 'API', true);
            return;
        }
        
        const data = await response.json();
        updateTrackInfo(data);
    } catch (error) {
        log(`Error fetching now playing: ${error.message}`, 'API', true);
    }
}

//
// UI Helpers
//

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

// Format time in mm:ss format
function formatTime(seconds) {
    if (!seconds) return '0:00';
    const minutes = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

// Show status message
function showStatus(message, isError = false, autoHide = true) {
    log(`Status: ${message}`, 'UI', isError);
    
    statusMessage.textContent = message;
    statusMessage.style.display = 'block';
    statusMessage.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
    
    if (!isError && autoHide) {
        setTimeout(() => {
            statusMessage.style.display = 'none';
        }, 3000);
    }
}

// Logging function
function log(message, category = 'INFO', isError = false) {
    const timestamp = new Date().toISOString().substr(11, 8);
    console[isError ? 'error' : 'log'](`[${timestamp}] [${category}] ${message}`);
}

// Initialize the player on document ready
document.addEventListener('DOMContentLoaded', initPlayer);(data.track);