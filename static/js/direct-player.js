// static/js/direct-player.js - Complete enhanced version with position persistence and better sync

// Configuration constants
const config = {
    NOW_PLAYING_INTERVAL: 8000,      // Check now playing every 8 seconds
    CONNECTION_CHECK_INTERVAL: 5000, // Check connection health every 5 seconds
    RECONNECT_ATTEMPTS: 8,           // Maximum reconnection attempts
    DEBUG_MODE: true,                // Enable for verbose logging
    
    // Error handling
    MAX_ERROR_FREQUENCY: 5000,       // Minimum time between error responses (ms)
    CLEANUP_DELAY: 300,              // Delay before cleaning up audio element
    RECONNECT_MIN_DELAY: 1000,       // Minimum reconnection delay
    RECONNECT_MAX_DELAY: 8000,       // Maximum reconnection delay
    
    // Track transition and position sync
    TRACK_CHANGE_GRACE_PERIOD: 2000, // Time to wait before reconnecting on track change
    POSITION_SYNC_TOLERANCE: 3,      // Seconds tolerance for position synchronization
    POSITION_SAVE_INTERVAL: 5000,    // How often to save position to localStorage
    
    // iOS-specific settings
    IOS_INTERACTION_TIMEOUT: 10000,  // How long to wait for user interaction on iOS
    IOS_RETRY_DELAY: 2000,           // Delay before retrying on iOS
};

// Enhanced player state with position persistence
const state = {
    // Audio element and management
    audioElement: null,
    cleanupTimeout: null,
    isCleaningUp: false,
    userHasInteracted: false,
    
    // Connection and status
    isPlaying: false,
    isMuted: false,
    volume: 0.7,
    lastTrackInfoTime: Date.now(),
    lastErrorTime: 0,
    reconnectAttempts: 0,
    isReconnecting: false,
    
    // Enhanced track info and position tracking
    currentTrackId: null,
    currentTrack: null,
    serverPosition: 0,
    serverPositionMs: 0,
    trackChangeDetected: false,
    trackChangeTime: 0,
    
    // Position persistence and synchronization
    lastKnownPosition: 0,
    positionSyncTime: 0,
    disconnectionTime: null,
    maxReconnectGap: 10000, // 10 seconds max gap for position continuity
    lastPositionSave: 0,
    positionDriftCorrection: 0,
    
    // Client-side position estimation
    clientStartTime: null,
    clientPositionOffset: 0,
    
    // Timers
    nowPlayingTimer: null,
    connectionHealthTimer: null,
    positionSaveTimer: null,
    
    // Platform detection
    isIOS: /iPad|iPhone|iPod/.test(navigator.userAgent) && !window.MSStream,
    isSafari: /^((?!chrome|android).)*safari/i.test(navigator.userAgent),
    isMobile: /Mobi|Android/i.test(navigator.userAgent),
    
    // iOS-specific state
    iosPlaybackUnlocked: false,
    pendingPlay: false,
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

// Initialize the player
function initPlayer() {
    log("Initializing ChillOut Radio player with enhanced position sync");
    log(`Platform: ${state.isMobile ? 'Mobile' : 'Desktop'}, iOS: ${state.isIOS}, Safari: ${state.isSafari}`);
    
    // Verify all required elements are present
    if (!startBtn || !muteBtn || !volumeControl || !statusMessage) {
        log("Critical UI elements missing!", 'INIT', true);
        alert("Player initialization failed: UI elements not found");
        return;
    }
    
    // Set up event listeners
    setupEventListeners();
    
    // iOS-specific initialization
    if (state.isIOS) {
        setupIOSOptimizations();
    }
    
    // Load saved settings and position
    loadSavedSettings();
    loadPositionFromStorage();
    
    // Set up position saving timer
    state.positionSaveTimer = setInterval(savePositionToStorage, config.POSITION_SAVE_INTERVAL);
    
    // Fetch initial track info
    fetchNowPlaying();
    
    log('ChillOut Radio player initialized successfully with position persistence');
    showStatus('Player ready - click Connect to start streaming', false, false);
}

// Set up event listeners
function setupEventListeners() {
    // Main connect/disconnect button
    startBtn.addEventListener('click', function(e) {
        e.preventDefault();
        state.userHasInteracted = true;
        toggleConnection();
    });
    
    // Mute button
    muteBtn.addEventListener('click', function(e) {
        e.preventDefault();
        state.userHasInteracted = true;
        
        state.isMuted = !state.isMuted;
        
        if (state.audioElement && !state.isCleaningUp) {
            state.audioElement.muted = state.isMuted;
        }
        
        muteBtn.textContent = state.isMuted ? 'Unmute' : 'Mute';
        
        try {
            localStorage.setItem('radioMuted', state.isMuted.toString());
        } catch (e) {
            // Ignore storage errors
        }
    });
    
    // Volume control
    volumeControl.addEventListener('input', function(e) {
        state.userHasInteracted = true;
        state.volume = this.value;
        
        if (state.audioElement && !state.isCleaningUp) {
            state.audioElement.volume = state.volume;
        }
        
        try {
            localStorage.setItem('radioVolume', this.value);
        } catch (e) {
            // Ignore storage errors
        }
    });
    
    // Listen for any user interaction to unlock iOS audio
    if (state.isIOS) {
        const unlockEvents = ['touchstart', 'touchend', 'click', 'keydown'];
        unlockEvents.forEach(eventType => {
            document.addEventListener(eventType, unlockIOSAudio, { once: true, passive: true });
        });
    }
}

// iOS-specific optimizations
function setupIOSOptimizations() {
    log("Setting up iOS optimizations with position sync", 'IOS');
    
    // Prevent iOS from sleeping during playback
    if ('wakeLock' in navigator) {
        navigator.wakeLock.request('screen').catch(err => {
            log(`Wake lock failed: ${err.message}`, 'IOS');
        });
    }
    
    // Set up visibility change handler for iOS app switching
    document.addEventListener('visibilitychange', function() {
        if (state.isPlaying && state.audioElement) {
            if (document.hidden) {
                log("App went to background, recording position", 'IOS');
                state.disconnectionTime = Date.now();
                state.lastKnownPosition = getCurrentEstimatedPosition();
            } else {
                log("App came to foreground, checking position sync", 'IOS');
                setTimeout(() => {
                    if (state.isPlaying && state.audioElement && state.audioElement.paused) {
                        log("Audio paused after background return, attempting recovery", 'IOS');
                        recoverIOSPlayback();
                    }
                }, 1000);
            }
        }
    });
}

// Unlock iOS audio on user interaction
function unlockIOSAudio(event) {
    if (state.iosPlaybackUnlocked) return;
    
    log("Attempting to unlock iOS audio", 'IOS');
    
    // Create a temporary audio element to unlock audio
    const tempAudio = new Audio();
    tempAudio.src = 'data:audio/mpeg;base64,SUQzBAAAAAAAI1RTU0UAAAAPAAADTGF2ZjU4Ljc2LjEwMAAAAAAAAAAAAAAA//OEAAAAAAAAAAAAAAAAAAAAAAAASW5mbwAAAA8AAAAEAAABIADAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMD/////////////////////wAAABhMYXZjNTguMTM=';
    
    const playPromise = tempAudio.play();
    if (playPromise !== undefined) {
        playPromise.then(() => {
            log("iOS audio unlocked successfully", 'IOS');
            state.iosPlaybackUnlocked = true;
            tempAudio.pause();
            tempAudio.src = '';
            
            if (state.pendingPlay && state.audioElement) {
                state.pendingPlay = false;
                attemptIOSPlay();
            }
        }).catch(err => {
            log(`iOS audio unlock failed: ${err.message}`, 'IOS', true);
        });
    }
}

// Enhanced position estimation
function getCurrentEstimatedPosition() {
    if (!state.clientStartTime) {
        return state.serverPosition;
    }
    
    const clientElapsed = (Date.now() - state.clientStartTime) / 1000;
    const estimatedPosition = state.clientPositionOffset + clientElapsed;
    
    // Apply any drift correction
    const correctedPosition = estimatedPosition + state.positionDriftCorrection;
    
    // Bound by track duration
    if (state.currentTrack && state.currentTrack.duration) {
        return Math.min(correctedPosition, state.currentTrack.duration);
    }
    
    return Math.max(0, correctedPosition);
}

// Calculate position drift and apply correction
function calculatePositionDrift(serverPosition, clientEstimate) {
    const drift = serverPosition - clientEstimate;
    const absDrift = Math.abs(drift);
    
    if (absDrift > config.POSITION_SYNC_TOLERANCE) {
        log(`Position drift detected: ${drift.toFixed(2)}s (server: ${serverPosition}s, client: ${clientEstimate.toFixed(2)}s)`, 'SYNC');
        
        // Apply gradual correction to avoid jarring jumps
        state.positionDriftCorrection += drift * 0.1; // 10% correction per update
        
        return true; // Significant drift detected
    }
    
    return false; // Within tolerance
}

// Load saved settings from localStorage
function loadSavedSettings() {
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
        log(`Error loading settings: ${e.message}`, 'INIT');
    }
}

// Load position from localStorage
function loadPositionFromStorage() {
    try {
        const saved = localStorage.getItem('radioPosition');
        if (saved) {
            const data = JSON.parse(saved);
            const age = Date.now() - data.timestamp;
            
            // Only use if less than 30 seconds old
            if (age < 30000) {
                state.lastKnownPosition = data.position + Math.floor(age / 1000);
                log(`Loaded saved position: ${state.lastKnownPosition}s (age: ${Math.floor(age/1000)}s)`, 'STORAGE');
                return data;
            }
        }
    } catch (e) {
        log(`Error loading position: ${e.message}`, 'STORAGE');
    }
    return null;
}

// Save position to localStorage
function savePositionToStorage() {
    try {
        if (state.currentTrackId && state.isPlaying) {
            const currentPos = getCurrentEstimatedPosition();
            const positionData = {
                trackId: state.currentTrackId,
                position: currentPos,
                timestamp: Date.now()
            };
            localStorage.setItem('radioPosition', JSON.stringify(positionData));
            state.lastPositionSave = Date.now();
            
            if (config.DEBUG_MODE) {
                log(`Saved position: ${currentPos.toFixed(1)}s`, 'STORAGE');
            }
        }
    } catch (e) {
        // Ignore storage errors
    }
}

// Start audio playback with enhanced position sync
function startAudio() {
    log('Starting audio playback with enhanced position sync', 'CONTROL');
    
    if (state.isPlaying || state.isReconnecting) {
        log('Already playing or reconnecting, ignoring start request', 'CONTROL');
        return;
    }
    
    startBtn.disabled = true;
    showStatus('Connecting to stream...', false, false);
    
    // Reset state
    state.isPlaying = true;
    state.isReconnecting = false;
    state.reconnectAttempts = 0;
    state.trackChangeDetected = false;
    state.pendingPlay = false;
    state.positionDriftCorrection = 0;
    
    // Get fresh track info and position before starting playback
    fetchNowPlaying().then(() => {
        log(`Starting playback with server position: ${state.serverPosition}s + ${state.serverPositionMs}ms`, 'CONTROL');
        
        // Initialize client position tracking
        state.clientStartTime = Date.now();
        state.clientPositionOffset = state.serverPosition;
        
        // Clean up existing audio element safely
        cleanupAudioElement().then(() => {
            createAudioElement();
            startDirectPlayback();
            setupTimers();
        });
    }).catch(() => {
        // If fetch fails, still try to start with current position
        log('Failed to fetch current position, using last known position', 'CONTROL');
        
        // Use saved position if available
        const savedPos = loadPositionFromStorage();
        if (savedPos && savedPos.trackId === state.currentTrackId) {
            state.serverPosition = savedPos.position;
            log(`Using saved position: ${state.serverPosition}s`, 'CONTROL');
        }
        
        state.clientStartTime = Date.now();
        state.clientPositionOffset = state.serverPosition;
        
        cleanupAudioElement().then(() => {
            createAudioElement();
            startDirectPlayback();
            setupTimers();
        });
    });
}

// Create audio element
function createAudioElement() {
    if (state.audioElement && !state.isCleaningUp) {
        log('Audio element already exists', 'AUDIO');
        return;
    }
    
    log('Creating new audio element', 'AUDIO');
    
    state.audioElement = new Audio();
    state.audioElement.controls = false;
    state.audioElement.volume = state.volume;
    state.audioElement.muted = state.isMuted;
    state.audioElement.crossOrigin = "anonymous";
    
    // iOS-specific settings
    if (state.isIOS) {
        state.audioElement.preload = 'auto';
        state.audioElement.playsInline = true;
    } else {
        state.audioElement.preload = 'auto';
    }
    
    // Set up audio event listeners
    setupAudioListeners();
    
    log('Audio element created and configured', 'AUDIO');
}

// Setup audio event listeners
function setupAudioListeners() {
    if (!state.audioElement) {
        log('No audio element to setup listeners on', 'AUDIO', true);
        return;
    }
    
    state.audioElement.addEventListener('playing', () => {
        log('Audio playing', 'AUDIO');
        showStatus('Audio playing');
        state.trackChangeDetected = false;
        state.pendingPlay = false;
        
        // Reset position tracking when playback starts
        state.clientStartTime = Date.now();
    });
    
    state.audioElement.addEventListener('waiting', () => {
        log('Audio buffering', 'AUDIO');
        showStatus('Buffering...', false, false);
    });
    
    state.audioElement.addEventListener('stalled', () => {
        log('Audio stalled', 'AUDIO');
        showStatus('Stream stalled - buffering', true, false);
        
        if (!state.isReconnecting && !state.trackChangeDetected) {
            setTimeout(() => {
                if (state.isPlaying && !state.isReconnecting && state.audioElement && state.audioElement.readyState < 3) {
                    log('Still stalled after delay, attempting reconnection', 'AUDIO');
                    attemptReconnection('stalled playback');
                }
            }, state.isIOS ? 5000 : 3000);
        }
    });
    
    state.audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        const errorMsg = getErrorMessage(e.target.error);
        
        log(`Audio error: ${errorMsg} (code ${errorCode})`, 'AUDIO', true);
        
        if (state.isPlaying && !state.isCleaningUp) {
            const now = Date.now();
            
            if (now - state.lastErrorTime > config.MAX_ERROR_FREQUENCY) {
                state.lastErrorTime = now;
                
                if (state.isIOS) {
                    handleIOSError(errorCode, errorMsg);
                } else {
                    handleStandardError(errorCode, errorMsg);
                }
            }
        }
    });
    
    state.audioElement.addEventListener('ended', () => {
        log('Audio ended', 'AUDIO');
        
        if (state.isPlaying && !state.isReconnecting) {
            if (state.trackChangeDetected) {
                log('Audio ended during track change, reconnecting to new track', 'AUDIO');
            } else {
                log('Audio ended unexpectedly, attempting to recover', 'AUDIO', true);
            }
            
            showStatus('Track ended - getting next track', false, false);
            attemptReconnection('track ended');
        }
    });
    
    // Enhanced progress monitoring with client-side estimation
    state.audioElement.addEventListener('timeupdate', () => {
        if (state.audioElement && !state.isCleaningUp && state.currentTrack && state.currentTrack.duration) {
            // Use client-side position estimation for smoother progress
            const estimatedPosition = getCurrentEstimatedPosition();
            updateProgressBar(estimatedPosition, state.currentTrack.duration);
        }
    });
}

// Handle iOS-specific errors
function handleIOSError(errorCode, errorMsg) {
    // Record position before handling error
    state.lastKnownPosition = getCurrentEstimatedPosition();
    state.disconnectionTime = Date.now();
    
    if (errorCode === 4) {
        if (state.trackChangeDetected && Date.now() - state.trackChangeTime < config.TRACK_CHANGE_GRACE_PERIOD) {
            log('iOS: Error during track change grace period, waiting...', 'IOS');
            return;
        }
        showStatus('iOS: Media format issue - reconnecting...', true, false);
    } else if (errorCode === 2) {
        showStatus('iOS: Network error - reconnecting...', true, false);
    } else {
        showStatus('iOS: Playback error - will reconnect', true, false);
    }
    
    setTimeout(() => {
        if (state.isPlaying && !state.isReconnecting) {
            attemptReconnection(`iOS error code ${errorCode}`);
        }
    }, config.IOS_RETRY_DELAY);
}

// Handle standard browser errors
function handleStandardError(errorCode, errorMsg) {
    // Record position before handling error
    state.lastKnownPosition = getCurrentEstimatedPosition();
    state.disconnectionTime = Date.now();
    
    if (errorCode === 4) {
        if (state.trackChangeDetected && Date.now() - state.trackChangeTime < config.TRACK_CHANGE_GRACE_PERIOD) {
            log('Error during track change grace period, waiting...', 'AUDIO');
            return;
        }
        showStatus('Media format error - reconnecting...', true, false);
    } else if (errorCode === 2) {
        showStatus('Network error - reconnecting...', true, false);
    } else {
        showStatus('Audio error - will try to reconnect', true, false);
    }
    
    setTimeout(() => {
        if (state.isPlaying && !state.isReconnecting) {
            attemptReconnection(`error code ${errorCode}`);
        }
    }, 1500);
}

// Clean up audio element safely
function cleanupAudioElement() {
    return new Promise((resolve) => {
        if (state.cleanupTimeout) {
            clearTimeout(state.cleanupTimeout);
            state.cleanupTimeout = null;
        }
        
        if (!state.audioElement) {
            resolve();
            return;
        }
        
        log('Cleaning up audio element', 'AUDIO');
        state.isCleaningUp = true;
        
        const elementToCleanup = state.audioElement;
        state.audioElement = null;
        
        try {
            elementToCleanup.pause();
        } catch (e) {
            log(`Error pausing during cleanup: ${e.message}`, 'AUDIO');
        }
        
        try {
            elementToCleanup.src = '';
            elementToCleanup.load();
        } catch (e) {
            log(`Error clearing source during cleanup: ${e.message}`, 'AUDIO');
        }
        
        state.cleanupTimeout = setTimeout(() => {
            try {
                if (elementToCleanup.parentNode) {
                    elementToCleanup.remove();
                }
            } catch (e) {
                log(`Error removing element during cleanup: ${e.message}`, 'AUDIO');
            }
            
            state.isCleaningUp = false;
            state.cleanupTimeout = null;
            resolve();
        }, config.CLEANUP_DELAY);
    });
}

// Setup timers
function setupTimers() {
    if (state.nowPlayingTimer) {
        clearInterval(state.nowPlayingTimer);
    }
    
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
    }
    
    state.nowPlayingTimer = setInterval(fetchNowPlaying, config.NOW_PLAYING_INTERVAL);
    state.connectionHealthTimer = setInterval(checkConnectionHealth, config.CONNECTION_CHECK_INTERVAL);
    
    log('Timers set up', 'CONTROL');
}

// Stop audio playback
function stopAudio(isError = false) {
    log(`Stopping audio playback${isError ? ' (due to error)' : ''}`, 'CONTROL');
    
    // Record disconnection time and position for continuity
    if (isError && state.isPlaying) {
        state.disconnectionTime = Date.now();
        state.lastKnownPosition = getCurrentEstimatedPosition();
        log(`Recorded disconnection at position ${state.lastKnownPosition.toFixed(1)}s`, 'SYNC');
    } else {
        state.disconnectionTime = null;
    }
    
    state.isPlaying = false;
    state.isReconnecting = false;
    state.pendingPlay = false;
    
    // Clear all timers
    if (state.nowPlayingTimer) {
        clearInterval(state.nowPlayingTimer);
        state.nowPlayingTimer = null;
    }
    
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
        state.connectionHealthTimer = null;
    }
    
    cleanupAudioElement().then(() => {
        log('Audio cleanup completed', 'CONTROL');
    });
    
    if (!isError) {
        showStatus('Disconnected from audio stream');
    }
    
    // Reset UI
    startBtn.textContent = 'Connect';
    startBtn.disabled = false;
    startBtn.dataset.connected = 'false';
    startBtn.onclick = toggleConnection;
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

// Start direct HTTP streaming with enhanced position synchronization
function startDirectPlayback() {
    if (!state.audioElement) {
        log('No audio element available for playback', 'PLAYBACK', true);
        return;
    }
    
    try {
        const timestamp = Date.now();
        
        // Determine the best position for synchronization
        let syncPosition = state.serverPosition;
        
        // If this is a reconnection within the gap threshold, try to use estimated position
        if (state.disconnectionTime && (timestamp - state.disconnectionTime) < state.maxReconnectGap) {
            const timeSinceDisconnect = (timestamp - state.disconnectionTime) / 1000;
            const estimatedPosition = state.lastKnownPosition + timeSinceDisconnect;
            
            // Use estimated position if it's reasonable and close to server position
            if (Math.abs(estimatedPosition - state.serverPosition) < config.POSITION_SYNC_TOLERANCE) {
                syncPosition = Math.floor(estimatedPosition);
                log(`Using continuity position: ${syncPosition}s (estimated: ${estimatedPosition.toFixed(1)}s, server: ${state.serverPosition}s)`, 'SYNC');
            } else {
                log(`Position gap too large, using server position: ${state.serverPosition}s (estimated: ${estimatedPosition.toFixed(1)}s)`, 'SYNC');
            }
        }
        
        // Create URL with enhanced position synchronization
        let streamUrl = `/direct-stream?t=${timestamp}`;
        
        // Always include position for synchronization
        streamUrl += `&position=${syncPosition}`;
        
        // Add platform info for optimized streaming
        if (state.isIOS) {
            streamUrl += '&platform=ios';
        } else if (state.isSafari) {
            streamUrl += '&platform=safari';
        } else if (state.isMobile) {
            streamUrl += '&platform=mobile';
        }
        
        log(`Using enhanced position-synchronized stream URL: ${streamUrl} (position: ${syncPosition}s)`, 'PLAYBACK');
        
        // Update client position tracking
        state.clientStartTime = Date.now();
        state.clientPositionOffset = syncPosition;
        state.disconnectionTime = null;
        
        // Set the audio source safely
        if (state.audioElement && !state.isCleaningUp) {
            state.audioElement.src = streamUrl;
        } else {
            log('Audio element not available when setting source', 'PLAYBACK', true);
            return;
        }
        
        // Handle playback based on platform
        if (state.isIOS) {
            handleIOSPlayback();
        } else {
            handleStandardPlayback();
        }
        
    } catch (e) {
        log(`Direct streaming error: ${e.message}`, 'AUDIO', true);
        showStatus(`Streaming error: ${e.message}`, true);
        stopAudio(true);
    }
}

// Handle iOS-specific playback
function handleIOSPlayback() {
    log('Handling iOS playback', 'IOS');
    
    if (!state.userHasInteracted) {
        showStatus('iOS: Please tap Connect to enable audio', false, false);
        startBtn.textContent = 'Enable Audio';
        startBtn.disabled = false;
        startBtn.dataset.connected = 'true';
        return;
    }
    
    if (!state.iosPlaybackUnlocked) {
        log('iOS audio not unlocked, attempting unlock', 'IOS');
        state.pendingPlay = true;
        unlockIOSAudio();
        showStatus('iOS: Preparing audio...', false, false);
        startBtn.textContent = 'Preparing...';
        startBtn.disabled = true;
        
        setTimeout(() => {
            if (state.pendingPlay) {
                state.pendingPlay = false;
                showStatus('iOS: Please try again', true, false);
                startBtn.textContent = 'Try Again';
                startBtn.disabled = false;
            }
        }, config.IOS_INTERACTION_TIMEOUT);
        return;
    }
    
    attemptIOSPlay();
}

// Attempt to play on iOS
function attemptIOSPlay() {
    if (!state.audioElement || state.isCleaningUp) {
        log('No audio element for iOS play attempt', 'IOS', true);
        return;
    }
    
    log('Attempting iOS play', 'IOS');
    showStatus('iOS: Starting playback...', false, false);
    startBtn.disabled = true;
    
    const playPromise = state.audioElement.play();
    if (playPromise !== undefined) {
        playPromise.then(() => {
            log('iOS playback started successfully', 'IOS');
            showStatus('Stream playing');
            startBtn.textContent = 'Disconnect';
            startBtn.disabled = false;
            startBtn.dataset.connected = 'true';
            startBtn.onclick = toggleConnection;
        }).catch(e => {
            log(`iOS play failed: ${e.message}`, 'IOS', true);
            
            if (e.name === 'NotAllowedError') {
                showStatus('iOS: Tap to start audio', true, false);
                startBtn.textContent = 'Tap to Play';
                startBtn.disabled = false;
                startBtn.onclick = function() {
                    state.userHasInteracted = true;
                    attemptIOSPlay();
                };
            } else {
                showStatus(`iOS playback error: ${e.message}`, true);
                startBtn.disabled = false;
                
                setTimeout(() => {
                    if (state.isPlaying && !state.isReconnecting) {
                        recoverIOSPlayback();
                    }
                }, config.IOS_RETRY_DELAY);
            }
        });
    }
}

// Recover iOS playback
function recoverIOSPlayback() {
    log('Attempting iOS playback recovery', 'IOS');
    
    if (!state.isPlaying || state.isReconnecting) return;
    
    if (state.audioElement && !state.audioElement.paused) {
        log('iOS: Audio already playing, no recovery needed', 'IOS');
        return;
    }
    
    if (state.audioElement) {
        log('iOS: Trying to resume existing audio', 'IOS');
        const playPromise = state.audioElement.play();
        if (playPromise !== undefined) {
            playPromise.then(() => {
                log('iOS: Resume successful', 'IOS');
                showStatus('Stream resumed');
            }).catch(e => {
                log(`iOS: Resume failed, will reconnect: ${e.message}`, 'IOS');
                attemptReconnection('iOS resume failed');
            });
        }
    } else {
        log('iOS: No audio element, will reconnect', 'IOS');
        attemptReconnection('iOS no audio element');
    }
}

// Handle standard browser playback
function handleStandardPlayback() {
    showStatus('Starting playback...', false, false);
    
    setTimeout(() => {
        if (!state.audioElement || !state.isPlaying || state.isCleaningUp) return;
        
        const playPromise = state.audioElement.play();
        if (playPromise !== undefined) {
            playPromise.then(() => {
                log('Direct stream playback started', 'AUDIO');
                showStatus('Connected to stream');
                startBtn.textContent = 'Disconnect';
                startBtn.disabled = false;
                startBtn.dataset.connected = 'true';
            }).catch(e => {
                log(`Direct stream playback error: ${e.message}`, 'AUDIO', true);
                
                if (e.name === 'NotAllowedError') {
                    handleUserInteractionRequired();
                } else {
                    showStatus(`Playback error: ${e.message}`, true);
                    startBtn.disabled = false;
                    
                    setTimeout(() => {
                        if (state.isPlaying && !state.isReconnecting) {
                            attemptReconnection('playback error');
                        }
                    }, 2000);
                }
            });
        }
    }, 200);
}

// Handle browser that requires user interaction
function handleUserInteractionRequired() {
    showStatus('Click play to start audio (browser requires user interaction)', true, false);
    startBtn.disabled = false;
    startBtn.dataset.connected = 'true';
    
    startBtn.onclick = function() {
        if (!state.audioElement || state.isCleaningUp) return;
        
        if (state.audioElement.paused && state.isPlaying) {
            startBtn.disabled = true;
            state.audioElement.play().then(() => {
                showStatus('Stream playing');
                startBtn.textContent = 'Disconnect';
                startBtn.disabled = false;
                startBtn.onclick = toggleConnection;
            }).catch(e => {
                log(`Play failed: ${e.message}`, 'AUDIO', true);
                showStatus(`Playback error: ${e.message}`, true);
                startBtn.disabled = false;
            });
        } else {
            stopAudio();
        }
    };
}

// Check connection health
function checkConnectionHealth() {
    if (!state.isPlaying || state.isReconnecting) return;
    
    const now = Date.now();
    const timeSinceLastTrackInfo = (now - state.lastTrackInfoTime) / 1000;
    
    if (timeSinceLastTrackInfo > config.NOW_PLAYING_INTERVAL / 1000) {
        fetchNowPlaying();
    }
    
    if (state.audioElement && !state.isCleaningUp) {
        if (state.isIOS) {
            checkIOSHealth();
        } else {
            checkStandardHealth();
        }
    }
}

// iOS-specific health checks
function checkIOSHealth() {
    if (state.audioElement.paused && state.isPlaying && !state.trackChangeDetected) {
        log('iOS: Audio is paused unexpectedly', 'IOS', true);
        
        const playPromise = state.audioElement.play();
        if (playPromise !== undefined) {
            playPromise.then(() => {
                log('iOS: Successfully resumed paused audio', 'IOS');
            }).catch(e => {
                log(`iOS: Resume failed, will reconnect: ${e.message}`, 'IOS');
                attemptReconnection('iOS unexpected pause');
            });
        }
    }
    
    if (state.audioElement.networkState === HTMLMediaElement.NETWORK_NO_SOURCE) {
        log('iOS: Audio has no source', 'IOS', true);
        recoverIOSPlayback();
    }
}

// Standard browser health checks
function checkStandardHealth() {
    if (state.audioElement.paused && state.isPlaying && !state.trackChangeDetected) {
        log('Audio is paused unexpectedly', 'HEALTH', true);
        attemptReconnection('unexpected pause');
    }
    
    if (state.audioElement.networkState === HTMLMediaElement.NETWORK_NO_SOURCE) {
        log('Audio has no source', 'HEALTH', true);
        attemptReconnection('no source');
    }
}

// Enhanced reconnection with position continuity
function attemptReconnection(reason = 'unknown') {
    if (state.isReconnecting) {
        log(`Reconnection already in progress, ignoring request (reason: ${reason})`, 'CONTROL');
        return;
    }
    
    if (!state.isPlaying) {
        log(`Not playing, ignoring reconnection request (reason: ${reason})`, 'CONTROL');
        return;
    }
    
    if (state.reconnectAttempts >= config.RECONNECT_ATTEMPTS) {
        log(`Maximum reconnection attempts (${config.RECONNECT_ATTEMPTS}) reached`, 'CONTROL', true);
        showStatus('Could not reconnect to server. Please try again later.', true);
        stopAudio(true);
        return;
    }
    
    // Record position and time for continuity
    state.lastKnownPosition = getCurrentEstimatedPosition();
    state.disconnectionTime = Date.now();
    
    state.isReconnecting = true;
    state.reconnectAttempts++;
    
    const baseDelay = Math.min(
        config.RECONNECT_MIN_DELAY * Math.pow(1.5, state.reconnectAttempts - 1), 
        config.RECONNECT_MAX_DELAY
    );
    
    const iosMultiplier = state.isIOS ? 1.5 : 1;
    const delay = (baseDelay * iosMultiplier) + (Math.random() * 500);
    
    log(`Reconnection attempt ${state.reconnectAttempts}/${config.RECONNECT_ATTEMPTS} in ${delay/1000}s (reason: ${reason}, pos: ${state.lastKnownPosition.toFixed(1)}s)`, 'CONTROL');
    showStatus(`Reconnecting (${state.reconnectAttempts}/${config.RECONNECT_ATTEMPTS})...`, true, false);
    
    cleanupAudioElement().then(() => {
        setTimeout(() => {
            if (!state.isPlaying) {
                state.isReconnecting = false;
                return;
            }
            
            log(`Executing reconnection attempt ${state.reconnectAttempts}`, 'CONTROL');
            
            createAudioElement();
            
            fetchNowPlaying().then(() => {
                if (state.isPlaying && state.audioElement) {
                    startDirectPlayback();
                }
                
                setTimeout(() => {
                    state.isReconnecting = false;
                }, 2000);
            }).catch(() => {
                if (state.isPlaying && state.audioElement) {
                    startDirectPlayback();
                }
                state.isReconnecting = false;
            });
        }, delay);
    });
}

// Enhanced track info update with position synchronization
function updateTrackInfo(info) {
    try {
        if (info.error) {
            showStatus(`Server error: ${info.error}`, true);
            return;
        }
        
        const previousTrackId = state.currentTrackId;
        state.currentTrack = info;
        
        // Enhanced position synchronization
        if (info.playback_position !== undefined) {
            const serverPosition = info.playback_position;
            const serverPositionMs = info.playback_position_ms || 0;
            const now = Date.now();
            
            // Calculate client-side estimated position
            const clientEstimate = getCurrentEstimatedPosition();
            
            // Check for position drift and apply correction
            const significantDrift = calculatePositionDrift(serverPosition, clientEstimate);
            
            // Handle reconnections with position continuity
            if (state.disconnectionTime && (now - state.disconnectionTime) < state.maxReconnectGap) {
                const timeSinceDisconnect = (now - state.disconnectionTime) / 1000;
                const continuityPosition = state.lastKnownPosition + timeSinceDisconnect;
                
                // Use continuity position if it's close to server position
                if (Math.abs(continuityPosition - serverPosition) < config.POSITION_SYNC_TOLERANCE) {
                    log(`Using position continuity: ${continuityPosition.toFixed(1)}s vs server: ${serverPosition}s`, 'SYNC');
                    state.serverPosition = Math.floor(continuityPosition);
                } else {
                    log(`Position gap too large for continuity, syncing to server: ${serverPosition}s`, 'SYNC');
                    state.serverPosition = serverPosition;
                    // Reset client tracking
                    state.clientStartTime = now;
                    state.clientPositionOffset = serverPosition;
                    state.positionDriftCorrection = 0;
                }
            } else {
                // Normal position update
                const oldPosition = state.serverPosition;
                state.serverPosition = serverPosition;
                state.serverPositionMs = serverPositionMs;
                
                if (significantDrift) {
                    log(`Applying position correction due to drift`, 'SYNC');
                    state.clientStartTime = now;
                    state.clientPositionOffset = serverPosition;
                }
                
                // Log significant position jumps for debugging
                if (config.DEBUG_MODE && Math.abs(oldPosition - serverPosition) > 2) {
                    log(`Server position update: ${oldPosition}s -> ${serverPosition}s`, 'SYNC');
                }
            }
            
            state.lastKnownPosition = state.serverPosition;
            state.lastTrackInfoTime = now;
            state.disconnectionTime = null;
        }
        
        // Track change detection
        const newTrackId = info.path;
        if (state.currentTrackId !== newTrackId) {
            log(`Track changed from "${previousTrackId}" to "${newTrackId}": ${info.title}`, 'TRACK');
            state.currentTrackId = newTrackId;
            
            state.trackChangeDetected = true;
            state.trackChangeTime = Date.now();
            
            // Reset position tracking for new track
            state.serverPosition = 0;
            state.serverPositionMs = 0;
            state.clientStartTime = Date.now();
            state.clientPositionOffset = 0;
            state.positionDriftCorrection = 0;
            
            if (state.isPlaying && state.audioElement && !state.isReconnecting) {
                const graceDelay = state.isIOS ? config.TRACK_CHANGE_GRACE_PERIOD * 1.5 : config.TRACK_CHANGE_GRACE_PERIOD;
                
                log(`Track changed while playing, will reconnect after ${graceDelay}ms grace period`, 'TRACK');
                
                setTimeout(() => {
                    if (state.isPlaying && state.trackChangeDetected && !state.isReconnecting) {
                        log("Grace period ended, reconnecting for new track", 'TRACK');
                        attemptReconnection('track change');
                    }
                }, graceDelay);
            }
        } else {
            state.trackChangeDetected = false;
        }
        
        // Update UI
        currentTitle.textContent = info.title || 'Unknown Title';
        currentArtist.textContent = info.artist || 'Unknown Artist';
        currentAlbum.textContent = info.album || 'Unknown Album';
        
        if (info.duration) {
            currentDuration.textContent = formatTime(info.duration);
        }
        
        // Update progress bar with enhanced position (client-side estimation)
        if (state.currentTrack && state.currentTrack.duration) {
            const displayPosition = getCurrentEstimatedPosition();
            updateProgressBar(displayPosition, info.duration);
        }
        
        if (info.active_listeners !== undefined) {
            listenerCount.textContent = `Listeners: ${info.active_listeners}`;
        }
        
        document.title = `${info.title} - ${info.artist} | ChillOut Radio`;
    } catch (e) {
        log(`Error processing track info: ${e.message}`, 'TRACK', true);
    }
}

// Fetch now playing info via API
async function fetchNowPlaying() {
    try {
        log("Fetching now playing information", 'API');
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            log(`Now playing API error: ${response.status}`, 'API', true);
            return null;
        }
        
        const data = await response.json();
        if (config.DEBUG_MODE) {
            log(`Received track info: ${data.title || 'Unknown'}`, 'API');
        }
        
        updateTrackInfo(data);
        return data;
    } catch (error) {
        log(`Error fetching now playing: ${error.message}`, 'API', true);
        return null;
    }
}

// Update the progress bar with smooth client-side estimation
function updateProgressBar(position, duration) {
    if (progressBar && duration > 0) {
        const percent = (position / duration) * 100;
        progressBar.style.width = `${percent}%`;
        
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
    if (config.DEBUG_MODE || isError) {
        log(`Status: ${message}`, 'UI', isError);
    }
    
    statusMessage.textContent = message;
    statusMessage.style.display = 'block';
    statusMessage.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
    
    if (!isError && autoHide) {
        setTimeout(() => {
            if (statusMessage.textContent === message) {
                statusMessage.style.display = 'none';
            }
        }, 3000);
    }
}

// Get human-readable error message
function getErrorMessage(error) {
    if (!error) return 'Unknown error';
    
    switch (error.code) {
        case MediaError.MEDIA_ERR_ABORTED:
            return 'Playback aborted';
        case MediaError.MEDIA_ERR_NETWORK:
            return 'Network error';
        case MediaError.MEDIA_ERR_DECODE:
            return 'Decoding error';
        case MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED:
            return 'Format not supported';
        default:
            return `Media error (code ${error.code})`;
    }
}

// Enhanced logging function
function log(message, category = 'INFO', isError = false) {
    if (isError || config.DEBUG_MODE) {
        const timestamp = new Date().toISOString().substr(11, 8);
        const style = isError 
            ? 'color: #e74c3c; font-weight: bold;' 
            : (category === 'IOS' ? 'color: #ff6b6b; font-weight: bold;' :
               category === 'AUDIO' ? 'color: #2ecc71;' : 
               category === 'CONTROL' ? 'color: #9b59b6;' : 
               category === 'TRACK' ? 'color: #f39c12;' : 
               category === 'SYNC' ? 'color: #3498db; font-weight: bold;' :
               category === 'STORAGE' ? 'color: #95a5a6;' : 'color: #2c3e50;');
        
        console[isError ? 'error' : 'log'](`%c[${timestamp}] [${category}] ${message}`, style);
    }
}

// Cleanup function for position save timer
function cleanup() {
    if (state.positionSaveTimer) {
        clearInterval(state.positionSaveTimer);
        state.positionSaveTimer = null;
    }
}

// Handle page unload
window.addEventListener('beforeunload', () => {
    savePositionToStorage();
    cleanup();
});

// Initialize the player when the document is ready
document.addEventListener('DOMContentLoaded', initPlayer);