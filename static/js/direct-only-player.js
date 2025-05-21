// Fixed direct-only-player.js with proper connection handling and debugging

// Player state and configuration
const state = {
    // Audio element 
    audioElement: null,
    
    // Connection and status
    isPlaying: false,
    isMuted: false,
    volume: 0.7,
    reconnectAttempts: 0,
    maxReconnectAttempts: 10,
    lastTrackInfoTime: Date.now(),
    lastStatusCheck: Date.now(),
    
    // Track info
    currentTrackId: null,
    lastKnownPosition: 0,
    serverPosition: 0,
    
    // Timers
    nowPlayingTimer: null,
    connectionHealthTimer: null,
    lastErrorTime: 0,
    cleanupTimeout: null,
    
    // Buffer monitoring
    lastPlaybackTime: 0,
    poorBufferStartTime: null,
    stalledStartTime: null,
    
    // Connection state
    connectionType: 'unknown',
    isReconnecting: false,
    
    // Playback state
    needsPositionSync: true,
    syncedToServerPosition: false,
    
    // Track history - to detect track changes
    currentTrack: null,
    lastTrack: null,
    
    // Debug mode - set to true while troubleshooting
    debugMode: true
};

// Configuration constants 
const config = {
    // Connection settings
    NOW_PLAYING_INTERVAL: 10000,      // Check now playing every 10 seconds
    CONNECTION_CHECK_INTERVAL: 3000,  // Check connection health more frequently
    STATUS_CHECK_INTERVAL: 30000,     // Check stream status from server
    
    // Buffer thresholds
    MIN_BUFFER_SECONDS: 3,            // Minimum buffer time in seconds
    POOR_BUFFER_THRESHOLD: 5000,      // Time with poor buffer before reconnect
    STALLED_THRESHOLD: 4000,          // Time stalled before reconnect
    
    // Reconnection settings
    MIN_RECONNECT_DELAY: 1000,        // Minimum reconnection delay
    MAX_RECONNECT_DELAY: 8000,        // Maximum reconnection delay
    RECONNECT_BACKOFF_FACTOR: 1.5,    // Exponential backoff factor
    
    // Position synchronization
    SYNC_POSITION_THRESHOLD: 10,      // How many seconds off before we force resync
    POSITION_CHECK_INTERVAL: 5000,    // How often to check if we're in sync
    
    // Debug settings
    SHOW_DEBUG_INFO: true,            // Show debug info in console
    LOG_BUFFER_INFO_FREQUENCY: 0.1    // How often to log buffer info (0-1)
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

// Detect platform for device-specific handling
const isAppleDevice = /iPad|iPhone|iPod|Mac/.test(navigator.userAgent) && !window.MSStream;
const isSafari = /^((?!chrome|android).)*safari/i.test(navigator.userAgent);
const isMobile = /Mobi|Android/i.test(navigator.userAgent);

// Initialize player
function initPlayer() {
    log("Initializing direct streaming radio player... Platform: " + (isMobile ? 'Mobile' : 'Desktop') + " (Apple: " + isAppleDevice + ", Safari: " + isSafari + ")", 'INIT');
    
    // Make sure all required DOM elements are available
    if (!startBtn || !muteBtn || !volumeControl || !statusMessage) {
        console.error("Critical error: Required UI elements not found!");
        alert("Player initialization error: UI elements not found.");
        return;
    }
    
    // Set up event listeners
    log("Setting up event listeners", 'INIT');
    startBtn.addEventListener('click', toggleConnection);
    
    muteBtn.addEventListener('click', function() {
        state.isMuted = !state.isMuted;
        
        if (state.audioElement) {
            state.audioElement.muted = state.isMuted;
        }
        
        muteBtn.textContent = state.isMuted ? 'Unmute' : 'Mute';
        
        try {
            localStorage.setItem('radioMuted', state.isMuted.toString());
        } catch (e) {
            // Ignore storage errors
        }
    });
    
    volumeControl.addEventListener('input', function() {
        state.volume = this.value;
        
        if (state.audioElement) {
            state.audioElement.volume = state.volume;
        }
        
        try {
            localStorage.setItem('radioVolume', this.value);
        } catch (e) {
            // Ignore storage errors
        }
    });
    
    // Load saved volume and mute state from localStorage
    try {
        const savedVolume = localStorage.getItem('radioVolume');
        if (savedVolume !== null) {
            volumeControl.value = savedVolume;
            state.volume = parseFloat(savedVolume);
        }
        
        const savedMuted = localStorage.getItem('radioMuted');
        if (savedMuted !== null) {
            state.isMuted = savedMuted === 'true';
            muteBtn.textContent = state.isMuted ? 'Unmute' : 'Mute';
        }
    } catch (e) {
        // Ignore storage errors
    }
    
    // Check network connection type if available
    if (navigator.connection) {
        state.connectionType = navigator.connection.effectiveType || 'unknown';
        log("Network connection type: " + state.connectionType);
        
        // Listen for connection changes
        if (navigator.connection.addEventListener) {
            navigator.connection.addEventListener('change', function() {
                const newType = navigator.connection.effectiveType || 'unknown';
                log("Network connection changed from " + state.connectionType + " to " + newType);
                state.connectionType = newType;
                
                // If connection improved significantly and we were having issues, try reconnecting
                if ((state.connectionType === '4g' || state.connectionType === '3g') && 
                    state.isPlaying && state.audioElement && state.audioElement.readyState < 3) {
                    log('Connection improved and playback was struggling - attempting reconnection');
                    attemptReconnection();
                }
            });
        }
    }
    
    // Check if browser has AudioContext support for better buffering detection
    state.hasAudioContextSupport = window.AudioContext || window.webkitAudioContext;
    
    // Fetch initial track info
    log("Fetching initial track info", 'INIT');
    fetchNowPlaying();
    
    // Initial stream status check
    fetchStreamStatus();
    
    log('ChillOut Radio player initialized');
    showStatus('Player ready - click Connect to start streaming', false, false);
}

// Setup audio listeners separately to keep organized
function setupAudioListeners() {
    log("Setting up audio event listeners", 'AUDIO');
    
    // Playback state events
    state.audioElement.addEventListener('loadstart', () => {
        log('Audio load started', 'AUDIO');
    });
    
    state.audioElement.addEventListener('loadedmetadata', () => {
        log('Audio metadata loaded', 'AUDIO');
    });
    
    state.audioElement.addEventListener('playing', () => {
        log('Audio playing', 'AUDIO');
        showStatus('Audio playing');
        state.poorBufferStartTime = null;
        state.stalledStartTime = null;
    });
    
    state.audioElement.addEventListener('waiting', () => {
        log('Audio buffering', 'AUDIO');
        showStatus('Buffering...', false, false);
        
        // Start tracking buffer waiting time
        if (!state.poorBufferStartTime) {
            state.poorBufferStartTime = Date.now();
        }
    });
    
    state.audioElement.addEventListener('stalled', () => {
        log('Audio stalled', 'AUDIO');
        showStatus('Stream stalled - buffering', true, false);
        
        // Start tracking stall time
        if (!state.stalledStartTime) {
            state.stalledStartTime = Date.now();
        }
        
        // If we're stalled for too long, try reconnecting
        const now = Date.now();
        
        // Only trigger reconnect if not already reconnecting and not too recent error
        if (!state.isReconnecting && now - state.lastErrorTime > 10000) {
            state.lastErrorTime = now;
            
            // Set a timeout to check if we're still stalled after a delay
            setTimeout(() => {
                // Only reconnect if still stalled and playing
                if (state.audioElement && state.stalledStartTime && 
                    Date.now() - state.stalledStartTime > config.STALLED_THRESHOLD && 
                    state.isPlaying) {
                    
                    log('Stream still stalled - attempting reconnection', 'AUDIO');
                    state.stalledStartTime = null; // Reset stall time
                    attemptReconnection();
                }
            }, config.STALLED_THRESHOLD);
        }
    });
    
    state.audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        let errorMsg = 'Unknown error';
        
        // Translate error codes to human-readable messages
        if (e.target.error) {
            switch(e.target.error.code) {
                case MediaError.MEDIA_ERR_ABORTED:
                    errorMsg = 'Playback aborted by user';
                    break;
                case MediaError.MEDIA_ERR_NETWORK:
                    errorMsg = 'Network error during download';
                    break;
                case MediaError.MEDIA_ERR_DECODE:
                    errorMsg = 'Decoding error - bad or corrupted file';
                    break;
                case MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED:
                    errorMsg = 'Format not supported by browser';
                    break;
            }
        }
        
        log("Audio error: " + errorMsg + " (code " + errorCode + ")", 'AUDIO', true);
        
        // Only react to errors if we're still trying to play
        if (state.isPlaying && !state.isReconnecting) {
            // Don't react to errors too frequently
            const now = Date.now();
            if (now - state.lastErrorTime > 10000) { // At most one error response per 10 seconds
                state.lastErrorTime = now;
                showStatus("Audio error - attempting to recover: " + errorMsg, true, false);
                
                // Try reconnecting with a short delay
                setTimeout(() => {
                    if (state.isPlaying && !state.isReconnecting) {
                        attemptReconnection();
                    }
                }, 500);
            }
        }
    });
    
    state.audioElement.addEventListener('ended', () => {
        log('Audio ended', 'AUDIO');
        // If we shouldn't be at the end, try to restart
        if (state.isPlaying && !state.isReconnecting) {
            log('Audio ended unexpectedly, attempting to recover', 'AUDIO', true);
            showStatus('Audio ended - reconnecting', true, false);
            attemptReconnection();
        }
    });
    
    // Buffering state events
    state.audioElement.addEventListener('canplay', () => {
        log('Audio can play', 'AUDIO');
        showStatus('Stream ready', false, true);
        
        // Reset buffer monitoring
        state.poorBufferStartTime = null;
    });
    
    state.audioElement.addEventListener('canplaythrough', () => {
        log('Audio can play through', 'AUDIO');
        // Clear any buffering messages
        if (statusMessage.textContent.includes('Buffering') || 
            statusMessage.textContent.includes('stalled')) {
            showStatus('Playback resumed', false, true);
        }
        
        // Reset all buffer monitoring
        state.poorBufferStartTime = null;
        state.stalledStartTime = null;
    });
    
    // Progress and seeking events
    state.audioElement.addEventListener('timeupdate', () => {
        if (state.audioElement) {
            // Update last known playback time for stall detection
            state.lastPlaybackTime = state.audioElement.currentTime;
            
            // Check if we're significantly out of sync with server position
            if (state.serverPosition > 0 && state.currentTrack) {
                // Calculate how far we are from server position
                const serverEstimatedPosition = state.serverPosition + 
                    ((Date.now() - state.lastTrackInfoTime) / 1000); // Adjust for time since update
                    
                const positionDiff = Math.abs(serverEstimatedPosition - state.audioElement.currentTime);
                
                // If we're too far off and not already syncing, consider reconnecting
                if (positionDiff > config.SYNC_POSITION_THRESHOLD && 
                    !state.syncedToServerPosition && !state.isReconnecting) {
                    log(`Position significantly out of sync: local=${state.audioElement.currentTime.toFixed(1)}s, server~=${serverEstimatedPosition.toFixed(1)}s, diff=${positionDiff.toFixed(1)}s`, 'SYNC', true);
                    
                    // If this is the first time we're detecting this, try to seek
                    if (state.needsPositionSync) {
                        tryPositionSync(serverEstimatedPosition);
                    }
                } else if (positionDiff < 5) {
                    // We're reasonably in sync
                    state.syncedToServerPosition = true;
                    state.needsPositionSync = false;
                }
            }
            
            // Reset stall timer during active playback
            state.stalledStartTime = null;
            
            // Update UI if we have duration info
            if (state.currentTrack && state.currentTrack.duration) {
                updateProgressBar(state.audioElement.currentTime, state.currentTrack.duration);
            }
        }
    });
    
    // Progress monitoring for stats
    state.audioElement.addEventListener('progress', () => {
        // Get buffer status
        const bufferInfo = getBufferInfo();
        if (config.SHOW_DEBUG_INFO && Math.random() < config.LOG_BUFFER_INFO_FREQUENCY) {
            log("Buffered " + bufferInfo.totalSeconds.toFixed(1) + " seconds of audio in " + bufferInfo.ranges + " ranges", 'BUFFER');
        }
        
        // Reset poor buffer monitoring when we receive data
        if (bufferInfo.totalSeconds > config.MIN_BUFFER_SECONDS) {
            state.poorBufferStartTime = null;
        }
    });
    
    // Seeking events
    state.audioElement.addEventListener('seeking', () => {
        log('Audio seeking to ' + state.audioElement.currentTime.toFixed(1) + 's', 'AUDIO');
    });
    
    state.audioElement.addEventListener('seeked', () => {
        log('Audio seeked to ' + state.audioElement.currentTime.toFixed(1) + 's', 'AUDIO');
    });
}

// Try to sync to the server position by seeking
function tryPositionSync(serverPosition) {
    if (!state.audioElement || !state.isPlaying || state.isReconnecting) {
        return false;
    }
    
    log(`Attempting to sync position to server time: ${serverPosition.toFixed(1)}s`, 'SYNC');
    
    try {
        // First check if we can seek in our current buffer
        const bufferInfo = getDetailedBufferInfo();
        let canSeekInBuffer = false;
        
        // Check if serverPosition is within any buffered range
        for (const range of bufferInfo.bufferedRanges) {
            if (serverPosition >= range.start && serverPosition <= range.end) {
                canSeekInBuffer = true;
                break;
            }
        }
        
        if (canSeekInBuffer) {
            log(`Server position ${serverPosition.toFixed(1)}s is within buffer, seeking...`, 'SYNC');
            state.audioElement.currentTime = serverPosition;
            state.needsPositionSync = false;
            state.syncedToServerPosition = true;
            return true;
        } else {
            // If we can't seek within buffer, we need to reconnect
            log(`Server position ${serverPosition.toFixed(1)}s is outside buffer, need to reconnect`, 'SYNC');
            state.needsPositionSync = true; // Still needs sync
            
            // Only reconnect if we're significantly out of sync
            if (Math.abs(serverPosition - state.audioElement.currentTime) > config.SYNC_POSITION_THRESHOLD) {
                // We'll try to reconnect and start from server position
                attemptReconnection(serverPosition);
                return true;
            }
        }
    } catch (e) {
        log(`Error during position sync: ${e.message}`, 'SYNC', true);
    }
    
    return false;
}

// Start audio playback
function startAudio() {
    log('Starting direct audio streaming', 'CONTROL');
    startBtn.disabled = true;
    showStatus('Connecting to stream...', false, false);
    
    // Reset state
    state.reconnectAttempts = 0;
    state.lastTrackInfoTime = Date.now();
    state.lastStatusCheck = Date.now();
    state.isPlaying = true;
    state.isReconnecting = false;
    state.syncedToServerPosition = false;
    state.needsPositionSync = true;
    
    // Clear any cleanup timeout that may be running
    if (state.cleanupTimeout) {
        clearTimeout(state.cleanupTimeout);
        state.cleanupTimeout = null;
    }
    
    // Clean up any existing element first
    cleanupAudioElement();
    
    // Create new audio element
    state.audioElement = new Audio();
    state.audioElement.controls = false;
    state.audioElement.volume = state.volume;
    state.audioElement.muted = state.isMuted;
    state.audioElement.preload = 'auto';
    
    // Platform specific settings
    if (isAppleDevice || isSafari) {
        // These properties can help with iOS playback
        state.audioElement.preload = 'auto';
        state.audioElement.autoplay = false;
    }
    
    // Add to document but hide visually
    state.audioElement.style.display = 'none';
    document.body.appendChild(state.audioElement);
    
    // Set up audio event listeners
    setupAudioListeners();
    
    // Start direct streaming
    startDirectPlayback();
    
    // Set up now playing update timer
    if (state.nowPlayingTimer) {
        clearInterval(state.nowPlayingTimer);
    }
    state.nowPlayingTimer = setInterval(fetchNowPlaying, config.NOW_PLAYING_INTERVAL);
    
    // Start connection health check timer
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
    }
    state.connectionHealthTimer = setInterval(checkConnectionHealth, config.CONNECTION_CHECK_INTERVAL);
    
    // Regular status check from server for better sync
    setInterval(fetchStreamStatus, config.STATUS_CHECK_INTERVAL);
}

// Stop audio playback with proper cleanup
function stopAudio(isError = false) {
    log("Stopping audio playback" + (isError ? ' (due to error)' : ''), 'CONTROL');
    
    // Set state flag first so other functions know we're stopping deliberately
    state.isPlaying = false;
    state.isReconnecting = false;
    
    // Clear all timers
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
        state.connectionHealthTimer = null;
    }
    
    if (state.nowPlayingTimer) {
        clearInterval(state.nowPlayingTimer);
        state.nowPlayingTimer = null;
    }
    
    // Clean up audio element with a slight delay
    cleanupAudioElement();
    
    if (!isError) {
        showStatus('Disconnected from audio stream');
    }
    
    // Reset UI
    startBtn.textContent = 'Connect';
    startBtn.disabled = false;
    startBtn.dataset.connected = 'false';
    
    // Reset event handlers
    startBtn.onclick = toggleConnection;
}

// Clean up audio element properly
function cleanupAudioElement() {
    // Cancel any scheduled cleanup
    if (state.cleanupTimeout) {
        clearTimeout(state.cleanupTimeout);
        state.cleanupTimeout = null;
    }
    
    // If we have an audio element, clean it up
    if (state.audioElement) {
        const element = state.audioElement;
        
        // Store a reference and clear the state variable first
        // This prevents other functions from trying to use it while we're cleaning up
        state.audioElement = null;
        
        try {
            // First pause playback
            element.pause();
        } catch (e) {
            // Ignore errors during pause
        }
        
        try {
            // Then clear source and load to flush the buffer
            element.src = '';
            element.load();
        } catch (e) {
            // Ignore errors during source clearing
        }
        
        // Schedule removal from DOM with delay to allow browser to clean up resources
        state.cleanupTimeout = setTimeout(() => {
            try {
                // Finally remove from DOM
                element.remove();
            } catch (e) {
                // Ignore errors during DOM removal
            }
            state.cleanupTimeout = null;
        }, 200);
    }
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

// Start direct HTTP streaming with improved platform detection and position sync
function startDirectPlayback(position) {
    try {
        // Get the current position from server first if not provided
        if (position === undefined) {
            log("Starting playback with fetched position", 'PLAYBACK');
            // Use what we already have
            const serverPosition = state.serverPosition || 0;
            setDirectPlaybackSource(serverPosition);
        } else {
            log(`Starting playback at specified position: ${position}s`, 'PLAYBACK');
            setDirectPlaybackSource(position);
        }
    } catch (e) {
        log("Direct streaming error: " + e.message, 'AUDIO', true);
        showStatus("Streaming error: " + e.message, true);
        stopAudio(true);
    }
}

// Set the audio element source with all required parameters
function setDirectPlaybackSource(position) {
    if (!state.audioElement || !state.isPlaying) return;
    
    const timestamp = Date.now();
    
    // Build URL with position and platform-specific parameters
    let streamUrl = `/direct-stream?t=${timestamp}&position=${position}`;
    
    // Add platform-specific parameters
    if (isAppleDevice) {
        streamUrl += '&platform=ios';
        
        // Request large buffer for iOS specifically
        streamUrl += '&buffer=large';
        
        // Add network quality hint
        if (state.connectionType && state.connectionType !== 'unknown') {
            streamUrl += "&network=" + state.connectionType;
        }
    } else if (isSafari) {
        streamUrl += '&platform=safari';
    } else if (isMobile) {
        streamUrl += '&platform=mobile';
    }
    
    log(`Using stream URL: ${streamUrl}`, 'PLAYBACK');
    
    // Apply the URL to the audio element
    state.audioElement.src = streamUrl;
    
    // Platform specific handling
    if (isAppleDevice) {
        // iOS/Safari requires user interaction to start playback
        showStatus('Ready - Tap play to start streaming', false, false);
        startBtn.textContent = 'Play';
        startBtn.disabled = false;
        startBtn.dataset.connected = 'true';
        
        // Setup click handler for playback that requires gentle handling on iOS
        startBtn.onclick = function() {
            if (!state.audioElement) return;
            
            if (state.audioElement.paused) {
                startBtn.disabled = true;
                showStatus('Starting playback...', false, false);
                
                // Add a short delay before playing on iOS
                setTimeout(() => {
                    if (!state.audioElement) return;
                    
                    state.audioElement.play().then(() => {
                        showStatus('Stream playing');
                        startBtn.textContent = 'Disconnect';
                        startBtn.disabled = false;
                        state.syncedToServerPosition = true;
                    }).catch(e => {
                        log("iOS play failed: " + e.message, 'AUDIO', true);
                        showStatus("Playback error: " + e.message, true);
                        startBtn.disabled = false;
                    });
                }, 100);
                
                // Reset onclick to normal toggle behavior after initial play
                startBtn.onclick = toggleConnection;
            } else {
                stopAudio();
            }
        };
    } else {
        // For other browsers, play automatically
        showStatus('Starting playback...', false, false);
        
        // Use a small delay before playing to ensure browser is ready
        setTimeout(() => {
            if (!state.audioElement || !state.isPlaying) return;
            
            const playPromise = state.audioElement.play();
            playPromise.then(() => {
                log('Direct stream playback started', 'AUDIO');
                showStatus('Connected to stream');
                startBtn.textContent = 'Disconnect';
                startBtn.disabled = false;
                startBtn.dataset.connected = 'true';
                state.syncedToServerPosition = true;
            }).catch(e => {
                log("Direct stream playback error: " + e.message, 'AUDIO', true);
                
                if (e.name === 'NotAllowedError') {
                    // Browser requires user interaction
                    showStatus('Click play to start audio (browser requires user interaction)', true, false);
                    startBtn.disabled = false;
                    startBtn.dataset.connected = 'true';
                    
                    // Setup click handler for playback
                    startBtn.onclick = function() {
                        if (!state.audioElement) return;
                        
                        if (state.audioElement.paused && state.isPlaying) {
                            startBtn.disabled = true;
                            state.audioElement.play().then(() => {
                                showStatus('Stream playing');
                                startBtn.textContent = 'Disconnect';
                                startBtn.disabled = false;
                                state.syncedToServerPosition = true;
                            }).catch(e => {
                                log("Play failed: " + e.message, 'AUDIO', true);
                                showStatus("Playback error: " + e.message, true);
                                startBtn.disabled = false;
                            });
                            // Reset onclick to normal toggle behavior after initial play
                            startBtn.onclick = toggleConnection;
                        } else {
                            stopAudio();
                        }
                    };
                } else {
                    showStatus("Playback error: " + e.message, true);
                    startBtn.disabled = false;
                    
                    // For other errors, try reconnecting
                    setTimeout(() => {
                        if (state.isPlaying && !state.isReconnecting) {
                            attemptReconnection();
                        }
                    }, 1000);
                }
            });
        }, 200);
    }
}

// Get detailed buffer info
function getDetailedBufferInfo() {
    if (!state.audioElement || !state.audioElement.buffered) {
        return {
            currentTime: 0,
            bufferedRanges: [],
            readyState: 0,
            networkState: 0
        };
    }
    
    const buffered = state.audioElement.buffered;
    const ranges = [];
    
    for (let i = 0; i < buffered.length; i++) {
        ranges.push({
            start: buffered.start(i),
            end: buffered.end(i),
            duration: buffered.end(i) - buffered.start(i)
        });
    }
    
    return {
        currentTime: state.audioElement.currentTime,
        bufferedRanges: ranges,
        readyState: state.audioElement.readyState,
        networkState: state.audioElement.networkState,
        paused: state.audioElement.paused,
        ended: state.audioElement.ended,
        seeking: state.audioElement.seeking
    };
}

// Check connection health with improved reliability and position sync
function checkConnectionHealth() {
    if (!state.isPlaying || state.isReconnecting) return;
    
    const now = Date.now();
    const timeSinceLastTrackInfo = (now - state.lastTrackInfoTime) / 1000;
    
    // Check if we need to update now playing info
    if (timeSinceLastTrackInfo > config.NOW_PLAYING_INTERVAL / 1000) {
        fetchNowPlaying();
    }
    
    // For direct streaming, perform thorough health check
    if (state.audioElement) {
        // Get detailed buffer info for better diagnosis
        const bufferInfo = getDetailedBufferInfo();
        
        // Log buffer status periodically for monitoring
        if (config.SHOW_DEBUG_INFO && Math.random() < 0.2) {
            log("Health: readyState=" + bufferInfo.readyState + ", networkState=" + bufferInfo.networkState + ", ranges=" + bufferInfo.bufferedRanges.length + ", paused=" + bufferInfo.paused, 'HEALTH');
        }
        
        // ISSUE #1: Paused unexpectedly
        if (state.audioElement.paused && state.isPlaying && !state.isReconnecting) {
            // Only if not a very recent error
            if (now - state.lastErrorTime > 10000) {
                log('Stream paused unexpectedly', 'HEALTH', true);
                showStatus('Stream interrupted. Reconnecting...', true, false);
                attemptReconnection();
                return;
            }
        }
        
        // ISSUE #2: Zero buffer and poor network state
        if (state.audioElement.readyState < 2 && 
            bufferInfo.bufferedRanges.length === 0 && 
            !state.isReconnecting) {
            
            log("Poor buffer state: readyState=" + state.audioElement.readyState + ", no buffered data", 'HEALTH', true);
            
            // If we've been in a poor buffer state for a while, reconnect
            if (!state.poorBufferStartTime) {
                state.poorBufferStartTime = now;
            } else if (now - state.poorBufferStartTime > config.POOR_BUFFER_THRESHOLD) {
                log('Persistent poor buffer - attempting reconnection', 'HEALTH', true);
                state.poorBufferStartTime = null; // Reset timer
                attemptReconnection();
                return;
            }
        } else {
            // Reset poor buffer timer if we're in a good state
            state.poorBufferStartTime = null;
        }
        
        // ISSUE #3: Stalled playback (current time not advancing despite adequate buffer)
        if (state.audioElement.readyState >= 3 && 
            !state.audioElement.paused && 
            state.lastPlaybackTime !== undefined && 
            state.audioElement.currentTime === state.lastPlaybackTime && 
            !state.isReconnecting) {
            
            if (!state.stalledStartTime) {
                state.stalledStartTime = now;
            } else if (now - state.stalledStartTime > config.STALLED_THRESHOLD) {
                log('Playback stalled despite having buffer - attempting reconnection', 'HEALTH', true);
                state.stalledStartTime = null;
                attemptReconnection();
                return;
            }
        } else {
            state.stalledStartTime = null;
        }
        
        // ISSUE #4: Buffer looping - detect if we're playing the same portion of audio repeatedly
        const bufferEnd = getBufferEnd();
        if (bufferEnd > 0 && state.audioElement.currentTime > 0) {
            // If we're close to the end of our buffer and not getting new data,
            // we might be about to loop
            if (bufferEnd - state.audioElement.currentTime < 2 && 
                bufferInfo.bufferedRanges.length === 1 && 
                bufferInfo.totalSeconds < 20) {
                
                log(`Possible buffer loop detected: currentTime=${state.audioElement.currentTime.toFixed(1)}, bufferEnd=${bufferEnd.toFixed(1)}`, 'HEALTH', true);
                
                // Check if position is significantly different from server position
                const serverEstimatedPosition = state.serverPosition + 
                    ((Date.now() - state.lastTrackInfoTime) / 1000);
                    
                if (Math.abs(serverEstimatedPosition - state.audioElement.currentTime) > 10) {
                    log(`Buffer loop confirmed: local=${state.audioElement.currentTime.toFixed(1)}s, server~=${serverEstimatedPosition.toFixed(1)}s`, 'HEALTH', true);
                    showStatus('Fixing buffer loop...', true, false);
                    attemptReconnection(serverEstimatedPosition);
                    return;
                }
            }
        }
        
        // Update last playback time
        state.lastPlaybackTime = state.audioElement.currentTime;
        
        // After a while, check if we're still in sync with server
        if (now - state.lastTrackInfoTime > config.POSITION_CHECK_INTERVAL && state.serverPosition > 0) {
            const serverEstimatedPosition = state.serverPosition + 
                ((now - state.lastTrackInfoTime) / 1000);
                
            const positionDiff = Math.abs(serverEstimatedPosition - state.audioElement.currentTime);
            
            // If we're significantly out of sync and it's been a while, get fresh data
            if (positionDiff > config.SYNC_POSITION_THRESHOLD && 
                now - state.lastStatusCheck > config.STATUS_CHECK_INTERVAL) {
                
                log(`Position check: local=${state.audioElement.currentTime.toFixed(1)}s, server~=${serverEstimatedPosition.toFixed(1)}s, diff=${positionDiff.toFixed(1)}s`, 'SYNC');
                fetchStreamStatus();
            }
        }
    }
}

// Get buffer information from audio element
function getBufferInfo() {
    if (!state.audioElement || !state.audioElement.buffered || state.audioElement.buffered.length === 0) {
        return { ranges: 0, totalSeconds: 0 };
    }
    
    let totalSeconds = 0;
    const buffered = state.audioElement.buffered;
    
    for (let i = 0; i < buffered.length; i++) {
        totalSeconds += buffered.end(i) - buffered.start(i);
    }
    
    return {
        ranges: buffered.length,
        totalSeconds
    };
}

// Get the end time of the last buffered range
function getBufferEnd() {
    if (!state.audioElement || !state.audioElement.buffered || state.audioElement.buffered.length === 0) {
        return 0;
    }
    
    const buffered = state.audioElement.buffered;
    return buffered.end(buffered.length - 1);
}

// Attempt reconnection with improved exponential backoff and recovery
function attemptReconnection(position) {
    // Set reconnection flag to prevent multiple reconnections
    if (state.isReconnecting) {
        log('Already reconnecting, skipping additional request', 'CONTROL');
        return;
    }
    
    // Don't try to reconnect if we're not supposed to be playing
    if (!state.isPlaying) return;
    
    // Set reconnecting state
    state.isReconnecting = true;
    
    // Store position to sync to, if provided
    const syncPosition = position || (state.serverPosition > 0 ? 
        state.serverPosition + ((Date.now() - state.lastTrackInfoTime) / 1000) : 0);
    
    // Check if we've reached the maximum attempts
    if (state.reconnectAttempts >= state.maxReconnectAttempts) {
        log("Maximum reconnection attempts (" + state.maxReconnectAttempts + ") reached", 'CONTROL', true);
        showStatus('Could not reconnect to server. Please try again later.', true);
        
        // Reset UI
        stopAudio(true);
        return;
    }
    
    // Increment attempts
    state.reconnectAttempts++;
    
    // Calculate delay with exponential backoff and a bit of randomness
    const baseDelay = Math.min(
        config.MIN_RECONNECT_DELAY * Math.pow(config.RECONNECT_BACKOFF_FACTOR, state.reconnectAttempts - 1), 
        config.MAX_RECONNECT_DELAY
    );
    const jitter = Math.random() * 1000; // Add up to 1 second of jitter
    const delay = baseDelay + jitter;
    
    log("Reconnection attempt " + state.reconnectAttempts + "/" + state.maxReconnectAttempts + " in " + (delay/1000).toFixed(1) + "s (position: " + syncPosition.toFixed(1) + "s)", 'CONTROL');
    showStatus("Reconnecting (" + state.reconnectAttempts + "/" + state.maxReconnectAttempts + ")...", true, false);
    
    // Clean up existing audio element first - THIS IS CRITICAL
    cleanupAudioElement();
    
    // Schedule reconnection
    setTimeout(() => {
        // Double check we're still supposed to be playing
        if (!state.isPlaying) {
            state.isReconnecting = false;
            return;
        }
        
        log("Starting reconnection attempt " + state.reconnectAttempts + " at position " + syncPosition.toFixed(1) + "s", 'CONTROL');
        
        // Create a new audio element
        state.audioElement = new Audio();
        state.audioElement.controls = false;
        state.audioElement.volume = state.volume;
        state.audioElement.muted = state.isMuted;
        state.audioElement.preload = 'auto';
        
        // Add to document but hide visually
        state.audioElement.style.display = 'none';
        document.body.appendChild(state.audioElement);
        
        // Setup event listeners on new element
        setupAudioListeners();
        
        // Try playback with fresh source at the specified position
        startDirectPlayback(syncPosition);
        
        // Reset reconnection state after a delay to allow the connection attempt to complete
        setTimeout(() => {
            state.isReconnecting = false;
        }, 5000);
    }, delay);
}

// Update track info from API
function updateTrackInfo(info) {
    try {
        // Check for error message
        if (info.error) {
            showStatus("Server error: " + info.error, true);
            return;
        }
        
        // Store the current track as previous
        if (state.currentTrack) {
            state.lastTrack = { ...state.currentTrack };
        }
        
        // Store server position for sync
        if (info.playback_position !== undefined) {
            state.serverPosition = info.playback_position;
            state.lastTrackInfoTime = Date.now();
        }
        
        // Store current track info
        state.currentTrack = info;
        
        // Store track ID for change detection
        const newTrackId = info.path;
        if (state.currentTrackId !== newTrackId) {
            log("Track changed to: " + info.title, 'TRACK');
            state.currentTrackId = newTrackId;
            
            // Reset position tracking on track change
            state.lastKnownPosition = 0;
            state.syncedToServerPosition = false;
            state.needsPositionSync = true;
            
            // If we're playing, we need to reconnect to get the new track
            if (state.isPlaying && state.audioElement && !state.isReconnecting) {
                log("Track changed, reconnecting to get new audio", 'TRACK');
                attemptReconnection(0); // Start from beginning of new track
            }
        }
        
        // Update UI
        currentTitle.textContent = info.title || 'Unknown Title';
        currentArtist.textContent = info.artist || 'Unknown Artist';
        currentAlbum.textContent = info.album || 'Unknown Album';
        
        // Update progress
        if (info.duration) {
            currentDuration.textContent = formatTime(info.duration);
        }
        
        // Update progress bar if we have position
        if (info.playback_position !== undefined && info.duration) {
            updateProgressBar(info.playback_position, info.duration);
        }
        
        // Update listener count
        if (info.active_listeners !== undefined) {
            listenerCount.textContent = "Listeners: " + info.active_listeners;
        }
        
        // Store track ID in DOM for future comparison
        currentTitle.dataset.trackId = info.path;
        
        // Update page title
        document.title = info.title + " - " + info.artist + " | ChillOut Radio";
    } catch (e) {
        log("Error processing track info: " + e.message, 'TRACK', true);
    }
}

// Fetch now playing info via API
async function fetchNowPlaying() {
    try {
        log("Fetching now playing information", 'API');
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            log("Now playing API error: " + response.status, 'API', true);
            return null;
        }
        
        const data = await response.json();
        log("Received now playing data: " + JSON.stringify(data), 'API');
        updateTrackInfo(data);
        return data;
    } catch (error) {
        log("Error fetching now playing: " + error.message, 'API', true);
        return null;
    }
}

// Fetch stream status from server for better sync
async function fetchStreamStatus() {
    try {
        log("Fetching stream status", 'API');
        const response = await fetch('/stream-status');
        if (!response.ok) {
            log("Stream status API error: " + response.status, 'API', true);
            return;
        }
        
        const data = await response.json();
        log("Received stream status: " + JSON.stringify(data), 'API');
        
        // Update last status check time
        state.lastStatusCheck = Date.now();
        
        // Check server status
        if (data.status !== 'streaming' && state.isPlaying) {
            log('Server reports stream is not playing, but we think it is', 'API', true);
            // Don't reconnect immediately, just log the discrepancy
        }
        
        // Store server position
        if (data.playback_position !== undefined) {
            state.serverPosition = data.playback_position;
            
            // Check if we're significantly out of sync
            if (state.audioElement && Math.abs(state.serverPosition - state.audioElement.currentTime) > config.SYNC_POSITION_THRESHOLD) {
                log(`Status check: local=${state.audioElement.currentTime.toFixed(1)}s, server=${state.serverPosition.toFixed(1)}s`, 'SYNC');
                
                // Consider syncing if we're playing and out of sync
                if (state.isPlaying && !state.audioElement.paused && !state.isReconnecting) {
                    state.needsPositionSync = true;
                }
            }
        }
        
        // If we have current track info, update our player
        if (data.current_track) {
            updateTrackInfo({
                ...data.current_track,
                active_listeners: data.active_listeners,
                playback_position: data.playback_position
            });
        }
    } catch (error) {
        log("Error fetching stream status: " + error.message, 'API', true);
    }
}

// Update the progress bar
function updateProgressBar(position, duration) {
    if (progressBar && duration > 0) {
        const percent = (position / duration) * 100;
        progressBar.style.width = percent + "%";
        
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
    return minutes + ":" + secs.toString().padStart(2, '0');
}

// Show status message
function showStatus(message, isError = false, autoHide = true) {
    log("Status: " + message, 'UI', isError);
    
    statusMessage.textContent = message;
    statusMessage.style.display = 'block';
    statusMessage.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
    
    if (!isError && autoHide) {
        setTimeout(() => {
            // Only hide if this is still the current message
            if (statusMessage.textContent === message) {
                statusMessage.style.display = 'none';
            }
        }, 3000);
    }
}

// Logging function with better formatting
function log(message, category = 'INFO', isError = false) {
    // Always log errors and important messages during debugging
    if (isError || state.debugMode) {
        const timestamp = new Date().toISOString().substr(11, 8);
        const style = isError 
            ? 'color: #e74c3c; font-weight: bold;' 
            : (category === 'AUDIO' ? 'color: #2ecc71;' : 
            (category === 'BUFFER' ? 'color: #3498db;' : 
                (category === 'CONTROL' ? 'color: #9b59b6;' : 
                (category === 'SYNC' ? 'color: #f39c12;' : 'color: #2c3e50;'))));
        
        console[isError ? 'error' : 'log']("%c[" + timestamp + "] [" + category + "] " + message, style);
    }
}

// Initialize the player on document ready
document.addEventListener('DOMContentLoaded', initPlayer);