// static/js/direct-player.js - Fixed mobile player with better connection management

// Mobile-optimized configuration
const config = {
    NOW_PLAYING_INTERVAL: 10000,        // Check every 10 seconds (battery friendly)
    CONNECTION_CHECK_INTERVAL: 8000,    // Check connection every 8 seconds
    RECONNECT_ATTEMPTS: 5,              // Reduced attempts for mobile
    DEBUG_MODE: true,
    
    // Mobile-friendly error handling
    MAX_ERROR_FREQUENCY: 8000,          // Longer time between error responses
    CLEANUP_DELAY: 500,                 // Longer cleanup delay for mobile
    RECONNECT_MIN_DELAY: 2000,          // Longer minimum delay
    RECONNECT_MAX_DELAY: 15000,         // Longer maximum delay
    
    // Track transition optimized for mobile
    TRACK_CHANGE_GRACE_PERIOD: 3000,    // Longer grace period
    POSITION_SYNC_TOLERANCE: 5,         // More lenient tolerance for mobile
    POSITION_SAVE_INTERVAL: 8000,       // Less frequent saving for battery
    
    // Mobile-specific timeouts
    MOBILE_BUFFER_TIMEOUT: 12000,       // Longer buffer timeout
    MOBILE_HEARTBEAT_INTERVAL: 15000,   // Heartbeat to keep connection alive
    STALE_CONNECTION_TIMEOUT: 30000,    // When to consider connection stale
};

// Enhanced mobile state management
const state = {
    // Audio and connection
    audioElement: null,
    cleanupTimeout: null,
    isCleaningUp: false,
    userHasInteracted: false,
    connectionId: null,              // Track our connection ID
    lastHeartbeat: 0,               // Last heartbeat sent
    connectionState: 'disconnected', // Add connection state
    
    // Connection status
    isPlaying: false,
    isMuted: false,
    volume: 0.7,
    lastTrackInfoTime: Date.now(),
    lastErrorTime: 0,
    reconnectAttempts: 0,
    isReconnecting: false,
    consecutiveErrors: 0,           // Track consecutive errors
    
    // Track info and position
    currentTrackId: null,
    currentTrack: null,
    serverPosition: 0,
    serverPositionMs: 0,
    trackChangeDetected: false,
    trackChangeTime: 0,
    
    // Position tracking with mobile optimization
    lastKnownPosition: 0,
    positionSyncTime: 0,
    disconnectionTime: null,
    maxReconnectGap: 15000,         // Longer gap allowance for mobile
    lastPositionSave: 0,
    positionDriftCorrection: 0,
    clientStartTime: null,
    clientPositionOffset: 0,
    
    // Timers
    nowPlayingTimer: null,
    connectionHealthTimer: null,
    positionSaveTimer: null,
    heartbeatTimer: null,           // New heartbeat timer
    
    // Enhanced platform detection
    isIOS: /iPad|iPhone|iPod/.test(navigator.userAgent) && !window.MSStream,
    isSafari: /^((?!chrome|android).)*safari/i.test(navigator.userAgent),
    isMobile: /Mobi|Android/i.test(navigator.userAgent),
    isAndroid: /Android/i.test(navigator.userAgent),
    androidVersion: navigator.userAgent.match(/Android (\d+)/)?.[1] || 'unknown',
    
    // Mobile-specific state
    backgroundTime: 0,              // Time when app went to background
    networkType: 'unknown',         // Current network type
    lowPowerMode: false,            // Whether device is in low power mode
    
    // iOS-specific
    iosPlaybackUnlocked: false,
    pendingPlay: false,
    
    // Android-specific  
    androidOptimized: false,
};

// UI Elements
const startBtn = document.getElementById('start-btn');
const muteBtn = document.getElementById('mute-btn');
const volumeControl = document.getElementById('volume');
const statusMessage = document.getElementById('status-message');
const listenerCount = document.getElementById('listener-count');
const currentTitle = document.getElementById('current-title');
const currentArtist = document.getElementById('current-artist');
const currentAlbum = document.getElementById('current-album');
const currentPosition = document.getElementById('current-position');
const currentDuration = document.getElementById('current-duration');
const progressBar = document.getElementById('progress-bar');

// Initialize the mobile-optimized player
function initPlayer() {
    log("Initializing mobile-optimized ChillOut Radio player");
    log(`Platform: ${state.isMobile ? 'Mobile' : 'Desktop'}, iOS: ${state.isIOS}, Android: ${state.isAndroid} (v${state.androidVersion}), Safari: ${state.isSafari}`);
    
    // Detect network conditions
    detectNetworkConditions();
    
    // Verify UI elements
    if (!startBtn || !muteBtn || !volumeControl || !statusMessage) {
        log("Critical UI elements missing!", 'ERROR', true);
        alert("Player initialization failed: UI elements not found");
        return;
    }
    
    // Set up event listeners
    setupEventListeners();
    
    // Platform-specific optimizations
    if (state.isAndroid) {
        setupAndroidOptimizations();
    } else if (state.isIOS) {
        setupIOSOptimizations();
    }
    
    // Load saved settings
    loadSavedSettings();
    loadPositionFromStorage();
    
    // Set up mobile-optimized timers
    setupMobileTimers();
    
    // Initial track info fetch
    fetchNowPlaying();
    
    // Set up background/foreground handling
    setupVisibilityHandling();
    
    log('Mobile-optimized player initialized successfully');
    showStatus('Player ready - tap Connect to start streaming', false, false);
}

// Detect network conditions for mobile optimization
function detectNetworkConditions() {
    if (navigator.connection) {
        const connection = navigator.connection;
        state.networkType = connection.effectiveType || 'unknown';
        
        log(`Network: ${state.networkType}, downlink: ${connection.downlink || 'unknown'} Mbps`);
        
        // Adjust timeouts based on connection quality
        if (state.networkType === '2g' || state.networkType === 'slow-2g') {
            config.MOBILE_BUFFER_TIMEOUT = 20000;
            config.NOW_PLAYING_INTERVAL = 15000;
            log("Slow connection detected, adjusting timeouts");
        } else if (state.networkType === '3g') {
            config.MOBILE_BUFFER_TIMEOUT = 15000;
            config.NOW_PLAYING_INTERVAL = 12000;
        }
        
        // Listen for connection changes
        connection.addEventListener('change', () => {
            const newType = connection.effectiveType;
            if (newType !== state.networkType) {
                log(`Connection changed: ${state.networkType} -> ${newType}`);
                state.networkType = newType;
                
                // Adjust behavior for new connection
                if (state.isPlaying && (newType === '2g' || newType === 'slow-2g')) {
                    showStatus('Slow connection detected - adjusting quality', false, true);
                }
            }
        });
    }
}

// Setup mobile-optimized timers
function setupMobileTimers() {
    // Position saving timer (less frequent for battery life)
    if (state.positionSaveTimer) clearInterval(state.positionSaveTimer);
    state.positionSaveTimer = setInterval(savePositionToStorage, config.POSITION_SAVE_INTERVAL);
    
    // Heartbeat timer to maintain connection
    if (state.heartbeatTimer) clearInterval(state.heartbeatTimer);
    state.heartbeatTimer = setInterval(sendHeartbeat, config.MOBILE_HEARTBEAT_INTERVAL);
    
    log('Mobile-optimized timers configured');
}

// Send heartbeat to maintain connection and update listener count
async function sendHeartbeat() {
    if (!state.isPlaying || !state.connectionId) return;
    
    try {
        const response = await fetch(`/api/heartbeat?connection_id=${state.connectionId}`, {
            method: 'GET',
            headers: {
                'Cache-Control': 'no-cache'
            }
        });
        
        if (response.ok) {
            state.lastHeartbeat = Date.now();
            
            // Update listener count from heartbeat response if available
            const data = await response.json();
            if (data.active_listeners !== undefined) {
                listenerCount.textContent = `Listeners: ${data.active_listeners}`;
            }
        }
    } catch (error) {
        log(`Heartbeat failed: ${error.message}`, 'CONNECTION');
    }
}

// Setup visibility handling for mobile apps
function setupVisibilityHandling() {
    document.addEventListener('visibilitychange', function() {
        if (document.hidden) {
            // App went to background
            state.backgroundTime = Date.now();
            log('App went to background', 'MOBILE');
            
            if (state.isPlaying) {
                // Save current position
                state.lastKnownPosition = getCurrentEstimatedPosition();
                savePositionToStorage();
                
                // Reduce timer frequency to save battery
                if (state.nowPlayingTimer) {
                    clearInterval(state.nowPlayingTimer);
                    state.nowPlayingTimer = setInterval(fetchNowPlaying, 30000); // 30 seconds in background
                }
            }
        } else {
            // App came to foreground
            if (state.backgroundTime > 0) {
                const backgroundDuration = Date.now() - state.backgroundTime;
                log(`App returned to foreground after ${Math.round(backgroundDuration/1000)}s`, 'MOBILE');
                
                if (state.isPlaying) {
                    // Check if audio is still playing
                    setTimeout(() => {
                        if (state.audioElement && state.audioElement.paused && state.isPlaying) {
                            log('Audio paused during background, attempting recovery', 'MOBILE');
                            attemptReconnection('background recovery');
                        } else {
                            // Restore normal timer frequency
                            setupPlayingTimers();
                            
                            // Fetch fresh position data
                            fetchNowPlaying();
                        }
                    }, 1000);
                }
            }
        }
    });
}

// Android-specific optimizations
function setupAndroidOptimizations() {
    log(`Setting up Android optimizations for version ${state.androidVersion}`, 'ANDROID');
    
    state.androidOptimized = true;
    
    // Android-specific wake lock
    if ('wakeLock' in navigator) {
        navigator.wakeLock.request('screen').catch(err => {
            log(`Android wake lock failed: ${err.message}`, 'ANDROID');
        });
    }
    
    // Handle Android-specific audio context issues
    if ('AudioContext' in window || 'webkitAudioContext' in window) {
        try {
            const AudioContextClass = window.AudioContext || window.webkitAudioContext;
            const audioContext = new AudioContextClass();
            if (audioContext.state === 'suspended') {
                document.addEventListener('touchstart', () => {
                    audioContext.resume().then(() => {
                        log('Android AudioContext resumed', 'ANDROID');
                    });
                }, { once: true });
            }
        } catch (e) {
            log(`Android AudioContext setup failed: ${e.message}`, 'ANDROID');
        }
    }
}

// iOS-specific optimizations  
function setupIOSOptimizations() {
    log("Setting up iOS optimizations", 'IOS');
    
    // iOS wake lock
    if ('wakeLock' in navigator) {
        navigator.wakeLock.request('screen').catch(err => {
            log(`iOS wake lock failed: ${err.message}`, 'IOS');
        });
    }
    
    // iOS audio unlock events
    const unlockEvents = ['touchstart', 'touchend', 'click', 'keydown'];
    unlockEvents.forEach(eventType => {
        document.addEventListener(eventType, unlockIOSAudio, { once: true, passive: true });
    });
}

// Set up event listeners
function setupEventListeners() {
    startBtn.addEventListener('click', function(e) {
        e.preventDefault();
        state.userHasInteracted = true;
        toggleConnection();
    });
    
    muteBtn.addEventListener('click', function(e) {
        e.preventDefault();
        state.userHasInteracted = true;
        
        state.isMuted = !state.isMuted;
        
        if (state.audioElement && !state.isCleaningUp) {
            state.audioElement.muted = state.isMuted;
    state.audioElement.crossOrigin = "anonymous";
    
    // Mobile-specific settings
    if (state.isMobile) {
        state.audioElement.preload = 'metadata'; // Less aggressive preloading for mobile
        state.audioElement.autoplay = false;
        
        // Mobile attributes
        if (state.audioElement.setAttribute) {
            state.audioElement.setAttribute('webkit-playsinline', 'true');
            state.audioElement.setAttribute('playsinline', 'true');
        }
        
        if (state.isIOS) {
            state.audioElement.playsInline = true;
        }
    } else {
        state.audioElement.preload = 'auto';
    }
    
    // Set up mobile-optimized audio event listeners
    setupMobileAudioListeners();
    
    log(`Mobile-optimized audio element created`, 'AUDIO');
}

// Setup mobile-optimized audio event listeners
function setupMobileAudioListeners() {
    if (!state.audioElement) return;
    
    state.audioElement.addEventListener('playing', () => {
        log('Audio playing', 'AUDIO');
        showStatus('Stream connected and playing');
        state.trackChangeDetected = false;
        state.pendingPlay = false;
        state.consecutiveErrors = 0; // Reset error count on success
        
        // Reset position tracking when playback starts
        state.clientStartTime = Date.now();
        
        // Send heartbeat to confirm connection
        if (state.connectionId) {
            sendHeartbeat();
        }
    });
    
    state.audioElement.addEventListener('waiting', () => {
        log('Audio buffering', 'AUDIO');
        showStatus('Buffering...', false, false);
    });
    
    state.audioElement.addEventListener('stalled', () => {
        log('Audio stalled', 'AUDIO');
        showStatus('Stream stalled - buffering', true, false);
        
        if (!state.isReconnecting && !state.trackChangeDetected) {
            // Mobile gets longer timeout for stalled audio
            const stalledTimeout = state.isMobile ? config.MOBILE_BUFFER_TIMEOUT : 5000;
            setTimeout(() => {
                if (state.isPlaying && !state.isReconnecting && state.audioElement && state.audioElement.readyState < 3) {
                    log('Still stalled after timeout, attempting reconnection', 'AUDIO');
                    attemptReconnection('stalled playback');
                }
            }, stalledTimeout);
        }
    });
    
    state.audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        const errorMsg = getErrorMessage(e.target.error);
        
        state.consecutiveErrors++;
        log(`Audio error: ${errorMsg} (code ${errorCode}, consecutive: ${state.consecutiveErrors})`, 'AUDIO', true);
        
        if (state.isPlaying && !state.isCleaningUp) {
            const now = Date.now();
            
            if (now - state.lastErrorTime > config.MAX_ERROR_FREQUENCY) {
                state.lastErrorTime = now;
                handleMobileError(errorCode, errorMsg);
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
    
    // Enhanced progress monitoring for mobile
    state.audioElement.addEventListener('timeupdate', () => {
        if (state.audioElement && !state.isCleaningUp && state.currentTrack && state.currentTrack.duration) {
            const estimatedPosition = getCurrentEstimatedPosition();
            updateProgressBar(estimatedPosition, state.currentTrack.duration);
        }
    });
    
    // Mobile-specific events
    state.audioElement.addEventListener('loadstart', () => {
        if (state.isMobile) {
            log('Mobile: Audio load started', 'MOBILE');
        }
    });
    
    state.audioElement.addEventListener('canplay', () => {
        if (state.isMobile) {
            log('Mobile: Audio can start playing', 'MOBILE');
        }
    });
}

// Mobile-optimized error handling
function handleMobileError(errorCode, errorMsg) {
    log(`Mobile error handler: code ${errorCode}, message: ${errorMsg}`, 'MOBILE', true);
    
    // Record position for continuity
    state.lastKnownPosition = getCurrentEstimatedPosition();
    state.disconnectionTime = Date.now();
    
    // Determine reconnection strategy based on error type and consecutive count
    let reconnectDelay = config.RECONNECT_MIN_DELAY;
    
    if (state.consecutiveErrors > 3) {
        // Too many consecutive errors, wait longer
        reconnectDelay = Math.min(config.RECONNECT_MAX_DELAY, reconnectDelay * state.consecutiveErrors);
        showStatus(`Multiple errors detected - waiting ${Math.round(reconnectDelay/1000)}s before retry`, true, false);
    } else if (errorCode === 4) { // MEDIA_ERR_SRC_NOT_SUPPORTED
        showStatus('Media format issue - getting fresh stream...', true, false);
        reconnectDelay = state.isMobile ? 3000 : 2000;
    } else if (errorCode === 2) { // MEDIA_ERR_NETWORK
        showStatus('Network error - reconnecting...', true, false);
        reconnectDelay = state.networkType === '2g' ? 5000 : (state.isMobile ? 3000 : 2000);
    } else {
        showStatus('Audio error - will reconnect', true, false);
        reconnectDelay = state.isMobile ? 3000 : 2000;
    }
    
    setTimeout(() => {
        if (state.isPlaying && !state.isReconnecting) {
            attemptReconnection(`error code ${errorCode}`);
        }
    }, reconnectDelay);
}

// Start audio with mobile optimization
function startAudio() {
    log('Starting mobile-optimized audio playback', 'CONTROL');
    
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
    state.consecutiveErrors = 0;
    
    // Get fresh track info and position
    fetchNowPlaying().then(() => {
        log(`Starting playback with server position: ${state.serverPosition}s + ${state.serverPositionMs}ms`, 'CONTROL');
        
        // Initialize client position tracking
        state.clientStartTime = Date.now();
        state.clientPositionOffset = state.serverPosition;
        
        // Clean up and create new audio element
        cleanupAudioElement().then(() => {
            createAudioElement();
            startMobileDirectPlayback();
            setupPlayingTimers();
        });
    }).catch(() => {
        // If fetch fails, still try to start with saved position
        log('Failed to fetch current position, using saved position', 'CONTROL');
        
        const savedPos = loadPositionFromStorage();
        if (savedPos && savedPos.trackId === state.currentTrackId) {
            state.serverPosition = savedPos.position;
            log(`Using saved position: ${state.serverPosition}s`, 'CONTROL');
        } else {
            state.serverPosition = 0;
            log('No reliable position data, starting from beginning', 'CONTROL');
        }
        
        state.clientStartTime = Date.now();
        state.clientPositionOffset = state.serverPosition;
        
        cleanupAudioElement().then(() => {
            createAudioElement();
            startMobileDirectPlayback();
            setupPlayingTimers();
        });
    });
}

// Mobile-optimized direct playback
function startMobileDirectPlayback() {
    if (!state.audioElement) {
        log('No audio element for mobile playback', 'MOBILE', true);
        return;
    }
    
    try {
        const timestamp = Date.now();
        const syncPosition = state.serverPosition;
        
        log(`Mobile streaming with position: ${syncPosition}s`, 'MOBILE');
        
        // Create mobile-optimized URL
        let streamUrl = `/direct-stream?t=${timestamp}&position=${syncPosition}`;
        
        // Add platform identification
        if (state.isAndroid) {
            streamUrl += '&platform=android';
        } else if (state.isIOS) {
            streamUrl += '&platform=ios';
        } else if (state.isMobile) {
            streamUrl += '&platform=mobile';
        }
        
        log(`Mobile stream URL: ${streamUrl}`, 'MOBILE');
        
        // Update client position tracking
        state.clientStartTime = Date.now();
        state.clientPositionOffset = syncPosition;
        state.disconnectionTime = null;
        
        // Set source
        state.audioElement.src = streamUrl;
        
        log('Starting mobile playback attempt', 'MOBILE');
        showStatus('Connecting to mobile stream...', false, false);
        
        // Mobile-specific playback with user interaction handling
        setTimeout(() => {
            if (state.audioElement && state.isPlaying && !state.isCleaningUp) {
                const playPromise = state.audioElement.play();
                if (playPromise !== undefined) {
                    playPromise.then(() => {
                        log(`Mobile playback started successfully at position ${syncPosition}s`, 'MOBILE');
                        showStatus('Mobile stream connected');
                        startBtn.textContent = 'Disconnect';
                        startBtn.disabled = false;
                        startBtn.dataset.connected = 'true';
                        
                        // Extract connection ID from response headers if possible
                        // This would require additional API call or header parsing
                        
                    }).catch(e => {
                        log(`Mobile playback failed: ${e.message}`, 'MOBILE', true);
                        handleMobilePlaybackFailure(e);
                    });
                }
            }
        }, state.isMobile ? 800 : 200); // Longer delay for mobile devices
        
    } catch (e) {
        log(`Mobile streaming setup error: ${e.message}`, 'MOBILE', true);
        showStatus(`Mobile streaming error: ${e.message}`, true);
        stopAudio(true);
    }
}

// Handle mobile playback failures
function handleMobilePlaybackFailure(error) {
    log(`Mobile playback failure: ${error.name} - ${error.message}`, 'MOBILE', true);
    
    if (error.name === 'NotAllowedError') {
        showStatus('Please tap to enable audio playback', true, false);
        startBtn.disabled = false;
        startBtn.textContent = 'Enable Audio';
        startBtn.onclick = function() {
            state.userHasInteracted = true;
            startMobileDirectPlayback();
        };
    } else {
        showStatus(`Mobile playback failed - ${error.message}`, true);
        startBtn.disabled = false;
        
        setTimeout(() => {
            if (state.isPlaying && !state.isReconnecting) {
                attemptReconnection('mobile playback failure');
            }
        }, state.isMobile ? 4000 : 2000);
    }
}

// Setup timers for active playback
function setupPlayingTimers() {
    if (state.nowPlayingTimer) {
        clearInterval(state.nowPlayingTimer);
    }
    
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
    }
    
    // Use mobile-optimized intervals
    const nowPlayingInterval = state.isMobile ? config.NOW_PLAYING_INTERVAL : 8000;
    const healthCheckInterval = state.isMobile ? config.CONNECTION_CHECK_INTERVAL : 5000;
    
    state.nowPlayingTimer = setInterval(fetchNowPlaying, nowPlayingInterval);
    state.connectionHealthTimer = setInterval(checkMobileConnectionHealth, healthCheckInterval);
    
    log(`Playing timers set up: nowPlaying=${nowPlayingInterval}ms, health=${healthCheckInterval}ms`, 'CONTROL');
}

// Mobile-optimized connection health check
function checkMobileConnectionHealth() {
    if (!state.isPlaying || state.isReconnecting) return;
    
    const now = Date.now();
    const timeSinceLastTrackInfo = (now - state.lastTrackInfoTime) / 1000;
    const timeSinceLastHeartbeat = (now - state.lastHeartbeat) / 1000;
    
    // Check if we need fresh track info
    if (timeSinceLastTrackInfo > config.NOW_PLAYING_INTERVAL / 1000) {
        fetchNowPlaying();
    }
    
    // Check if heartbeat is too old
    if (timeSinceLastHeartbeat > config.MOBILE_HEARTBEAT_INTERVAL / 1000 * 2) {
        sendHeartbeat();
    }
    
    if (state.audioElement && !state.isCleaningUp) {
        // Mobile-specific health checks
        if (state.audioElement.paused && state.isPlaying && !state.trackChangeDetected) {
            log('Mobile: Audio is paused unexpectedly', 'MOBILE', true);
            
            const playPromise = state.audioElement.play();
            if (playPromise !== undefined) {
                playPromise.then(() => {
                    log('Mobile: Successfully resumed paused audio', 'MOBILE');
                }).catch(e => {
                    log(`Mobile: Resume failed, will reconnect: ${e.message}`, 'MOBILE');
                    attemptReconnection('mobile unexpected pause');
                });
            }
        }
        
        if (state.audioElement.networkState === HTMLMediaElement.NETWORK_NO_SOURCE) {
            log('Mobile: Audio has no source', 'MOBILE', true);
            attemptReconnection('mobile no source');
        }
        
        // Check for readyState issues on mobile
        if (state.audioElement.readyState < 2 && !state.trackChangeDetected) {
            log('Mobile: Audio readyState indicates loading issues', 'MOBILE');
            // Don't immediately reconnect, but prepare for it
        }
    }
}

// Mobile-optimized reconnection
function attemptReconnection(reason = 'unknown') {
    if (state.isReconnecting) {
        log(`Mobile reconnection already in progress, ignoring request (reason: ${reason})`, 'MOBILE');
        return;
    }
    
    if (!state.isPlaying) {
        log(`Not playing, ignoring reconnection request (reason: ${reason})`, 'MOBILE');
        return;
    }
    
    if (state.reconnectAttempts >= config.RECONNECT_ATTEMPTS) {
        log(`Maximum reconnection attempts (${config.RECONNECT_ATTEMPTS}) reached`, 'MOBILE', true);
        showStatus('Could not reconnect to server. Please try again later.', true);
        stopAudio(true);
        return;
    }
    
    // Record position and time for continuity
    state.lastKnownPosition = getCurrentEstimatedPosition();
    state.disconnectionTime = Date.now();
    
    state.isReconnecting = true;
    state.reconnectAttempts++;
    
    // Mobile-friendly exponential backoff
    const baseDelay = Math.min(
        config.RECONNECT_MIN_DELAY * Math.pow(1.3, state.reconnectAttempts - 1), 
        config.RECONNECT_MAX_DELAY
    );
    
    // Adjust delay for network conditions
    let networkMultiplier = 1;
    if (state.networkType === '2g' || state.networkType === 'slow-2g') {
        networkMultiplier = 2;
    } else if (state.networkType === '3g') {
        networkMultiplier = 1.5;
    }
    
    const delay = (baseDelay * networkMultiplier) + (Math.random() * 1000);
    
    log(`Mobile reconnection attempt ${state.reconnectAttempts}/${config.RECONNECT_ATTEMPTS} in ${Math.round(delay/1000)}s (reason: ${reason})`, 'MOBILE');
    showStatus(`Reconnecting (${state.reconnectAttempts}/${config.RECONNECT_ATTEMPTS})...`, true, false);
    
    cleanupAudioElement().then(() => {
        setTimeout(() => {
            if (!state.isPlaying) {
                state.isReconnecting = false;
                return;
            }
            
            log(`Executing mobile reconnection attempt ${state.reconnectAttempts}`, 'MOBILE');
            
            createAudioElement();
            
            fetchNowPlaying().then(() => {
                if (state.isPlaying && state.audioElement) {
                    startMobileDirectPlayback();
                }
                
                setTimeout(() => {
                    state.isReconnecting = false;
                }, 3000);
            }).catch(() => {
                if (state.isPlaying && state.audioElement) {
                    startMobileDirectPlayback();
                }
                state.isReconnecting = false;
            });
        }, delay);
    });
}

// Enhanced cleanup for mobile
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
        
        log('Cleaning up mobile audio element', 'MOBILE');
        state.isCleaningUp = true;
        
        const elementToCleanup = state.audioElement;
        state.audioElement = null;
        
        try {
            elementToCleanup.pause();
        } catch (e) {
            log(`Error pausing during cleanup: ${e.message}`, 'MOBILE');
        }
        
        try {
            elementToCleanup.src = '';
            elementToCleanup.load();
        } catch (e) {
            log(`Error clearing source during cleanup: ${e.message}`, 'MOBILE');
        }
        
        // Mobile may need longer cleanup time
        const cleanupDelay = state.isMobile ? config.CLEANUP_DELAY * 2 : config.CLEANUP_DELAY;
        
        state.cleanupTimeout = setTimeout(() => {
            try {
                if (elementToCleanup.parentNode) {
                    elementToCleanup.remove();
                }
            } catch (e) {
                log(`Error removing element during cleanup: ${e.message}`, 'MOBILE');
            }
            
            state.isCleaningUp = false;
            state.cleanupTimeout = null;
            resolve();
        }, cleanupDelay);
    });
}

// Stop audio with mobile cleanup
function stopAudio(isError = false) {
    log(`Stopping mobile audio playback${isError ? ' (due to error)' : ''}`, 'CONTROL');
    
    // Record disconnection for continuity
    if (isError && state.isPlaying) {
        state.disconnectionTime = Date.now();
        state.lastKnownPosition = getCurrentEstimatedPosition();
        log(`Recorded mobile disconnection at position ${state.lastKnownPosition.toFixed(1)}s`, 'MOBILE');
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
        log('Mobile audio cleanup completed', 'CONTROL');
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

// Enhanced position estimation
function getCurrentEstimatedPosition() {
    if (!state.clientStartTime) {
        return state.serverPosition;
    }
    
    const clientElapsed = (Date.now() - state.clientStartTime) / 1000;
    let estimatedPosition = state.clientPositionOffset + clientElapsed;
    
    // Apply drift correction
    if (state.positionDriftCorrection !== 0) {
        estimatedPosition += state.positionDriftCorrection;
    }
    
    // Bound by track duration
    if (state.currentTrack && state.currentTrack.duration) {
        estimatedPosition = Math.min(estimatedPosition, state.currentTrack.duration);
    }
    
    return Math.max(0, estimatedPosition);
}

// Fetch now playing with mobile optimization
async function fetchNowPlaying() {
    try {
        log("Fetching now playing information", 'API');
        
        let apiUrl = '/api/now-playing';
        if (state.isMobile) {
            apiUrl += '?mobile_client=true';
        }
        
        const response = await fetch(apiUrl, {
            headers: {
                'Cache-Control': 'no-cache'
            }
        });
        
        if (!response.ok) {
            log(`Now playing API error: ${response.status}`, 'API', true);
            return null;
        }
        
        const data = await response.json();
        updateTrackInfo(data);
        return data;
    } catch (error) {
        log(`Error fetching now playing: ${error.message}`, 'API', true);
        return null;
    }
}

// Update track info with mobile optimization
function updateTrackInfo(info) {
    try {
        if (info.error) {
            showStatus(`Server error: ${info.error}`, true);
            return;
        }
        
        const previousTrackId = state.currentTrackId;
        state.currentTrack = info;
        
        // Position synchronization
        if (info.playback_position !== undefined) {
            const serverPosition = info.playback_position;
            const serverPositionMs = info.playback_position_ms || 0;
            const now = Date.now();
            
            const clientEstimate = getCurrentEstimatedPosition();
            const drift = serverPosition - clientEstimate;
            
            // Mobile-friendly drift handling (more lenient)
            if (Math.abs(drift) > config.POSITION_SYNC_TOLERANCE) {
                log(`Mobile position drift: ${drift.toFixed(2)}s (server: ${serverPosition}s, client: ${clientEstimate.toFixed(2)}s)`, 'MOBILE');
                state.positionDriftCorrection += drift * 0.08; // Gentler correction for mobile
            }
            
            state.serverPosition = serverPosition;
            state.serverPositionMs = serverPositionMs;
            state.lastKnownPosition = serverPosition;
            state.lastTrackInfoTime = now;
            state.disconnectionTime = null;
        }
        
        // Track change detection
        const newTrackId = info.path;
        if (state.currentTrackId !== newTrackId) {
            log(`Track changed: ${info.title}`, 'TRACK');
            state.currentTrackId = newTrackId;
            state.trackChangeDetected = true;
            state.trackChangeTime = Date.now();
            
            // Reset position tracking for new track
            state.serverPosition = 0;
            state.clientStartTime = Date.now();
            state.clientPositionOffset = 0;
            state.positionDriftCorrection = 0;
            
            if (state.isPlaying && state.audioElement && !state.isReconnecting) {
                log("Track changed while playing, will reconnect after grace period", 'TRACK');
                
                setTimeout(() => {
                    if (state.isPlaying && state.trackChangeDetected && !state.isReconnecting) {
                        log("Grace period ended, reconnecting for new track", 'TRACK');
                        attemptReconnection('track change');
                    }
                }, config.TRACK_CHANGE_GRACE_PERIOD);
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
        
        // Update progress bar
        if (state.currentTrack && state.currentTrack.duration) {
            const displayPosition = getCurrentEstimatedPosition();
            updateProgressBar(displayPosition, info.duration);
        }
        
        // Update listener count from server response
        if (info.active_listeners !== undefined) {
            listenerCount.textContent = `Listeners: ${info.active_listeners}`;
        }
        
        document.title = `${info.title} - ${info.artist} | ChillOut Radio`;
    } catch (e) {
        log(`Error processing track info: ${e.message}`, 'TRACK', true);
    }
}

// Unlock iOS audio
function unlockIOSAudio(event) {
    if (state.iosPlaybackUnlocked) return;
    
    log("Attempting to unlock iOS audio", 'IOS');
    
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
                startMobileDirectPlayback();
            }
        }).catch(err => {
            log(`iOS audio unlock failed: ${err.message}`, 'IOS', true);
        });
    }
}

// Update progress bar
function updateProgressBar(position, duration) {
    if (progressBar && duration > 0) {
        const percent = (position / duration) * 100;
        progressBar.style.width = `${percent}%`;
        
        if (currentPosition) currentPosition.textContent = formatTime(position);
        if (currentDuration) currentDuration.textContent = formatTime(duration);
    }
}

// Format time
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
        }, 4000); // Longer display time for mobile
    }
}

// Get error message
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

// Enhanced logging
function log(message, category = 'INFO', isError = false) {
    if (isError || config.DEBUG_MODE) {
        const timestamp = new Date().toISOString().substr(11, 8);
        const style = isError 
            ? 'color: #e74c3c; font-weight: bold;' 
            : (category === 'MOBILE' ? 'color: #4CAF50; font-weight: bold;' :
               category === 'ANDROID' ? 'color: #FF9800; font-weight: bold;' :
               category === 'IOS' ? 'color: #ff6b6b; font-weight: bold;' :
               category === 'AUDIO' ? 'color: #2ecc71;' : 
               category === 'CONTROL' ? 'color: #9b59b6;' : 
               category === 'TRACK' ? 'color: #f39c12;' : 
               category === 'API' ? 'color: #3498db;' :
               category === 'CONNECTION' ? 'color: #1abc9c;' :
               category === 'STORAGE' ? 'color: #95a5a6;' : 'color: #2c3e50;');
        
        console[isError ? 'error' : 'log'](`%c[${timestamp}] [${category}] ${message}`, style);
    }
}

// Position drift calculation for mobile
function calculatePositionDrift(serverPosition, clientEstimate) {
    const drift = serverPosition - clientEstimate;
    const absDrift = Math.abs(drift);
    
    // Mobile devices get more lenient drift tolerance
    const tolerance = state.isMobile ? 6 : config.POSITION_SYNC_TOLERANCE;
    
    if (absDrift > tolerance) {
        log(`Position drift detected: ${drift.toFixed(2)}s (server: ${serverPosition}s, client: ${clientEstimate.toFixed(2)}s)`, 'SYNC');
        
        // Gentler correction for mobile devices
        const correctionFactor = state.isMobile ? 0.06 : 0.1;
        state.positionDriftCorrection += drift * correctionFactor;
        
        return true;
    }
    
    return false;
}

// Handle iOS-specific playback (if iOS detected)
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
        }, 10000);
        return;
    }
    
    attemptIOSPlay();
}

// Attempt iOS play
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
                        attemptReconnection('iOS play failed');
                    }
                }, 2000);
            }
        });
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

// Handle browser requiring user interaction
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

// Mobile-optimized direct playback startup
function startDirectPlayback() {
    // Mobile devices use their own optimized method
    if (state.isMobile) {
        return startMobileDirectPlayback();
    }
    
    if (!state.audioElement) {
        log('No audio element available for playback', 'PLAYBACK', true);
        return;
    }
    
    try {
        const timestamp = Date.now();
        let syncPosition = state.serverPosition;
        
        // Desktop position continuity logic
        if (state.disconnectionTime && (timestamp - state.disconnectionTime) < state.maxReconnectGap) {
            const timeSinceDisconnect = (timestamp - state.disconnectionTime) / 1000;
            const estimatedPosition = state.lastKnownPosition + timeSinceDisconnect;
            
            if (Math.abs(estimatedPosition - state.serverPosition) < config.POSITION_SYNC_TOLERANCE) {
                syncPosition = Math.floor(estimatedPosition);
                log(`Using continuity position: ${syncPosition}s`, 'SYNC');
            }
        }
        
        let streamUrl = `/direct-stream?t=${timestamp}&position=${syncPosition}`;
        
        if (state.isSafari) {
            streamUrl += '&platform=safari';
        }
        
        log(`Desktop stream URL: ${streamUrl}`, 'PLAYBACK');
        
        state.clientStartTime = Date.now();
        state.clientPositionOffset = syncPosition;
        state.disconnectionTime = null;
        
        if (state.audioElement && !state.isCleaningUp) {
            state.audioElement.src = streamUrl;
        }
        
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

// Network quality detection and adaptation
function adaptToNetworkQuality() {
    if (!navigator.connection) return;
    
    const connection = navigator.connection;
    const effectiveType = connection.effectiveType;
    
    // Adjust configuration based on network quality
    switch (effectiveType) {
        case 'slow-2g':
        case '2g':
            config.NOW_PLAYING_INTERVAL = 20000;
            config.MOBILE_BUFFER_TIMEOUT = 25000;
            config.RECONNECT_MIN_DELAY = 5000;
            log('Adapted to slow network (2G)', 'NETWORK');
            break;
        case '3g':
            config.NOW_PLAYING_INTERVAL = 15000;
            config.MOBILE_BUFFER_TIMEOUT = 18000;
            config.RECONNECT_MIN_DELAY = 3000;
            log('Adapted to 3G network', 'NETWORK');
            break;
        case '4g':
        default:
            // Use default values
            log('Using default network settings (4G+)', 'NETWORK');
            break;
    }
}

// Battery status detection (if available)
function handleBatteryStatus() {
    if ('getBattery' in navigator) {
        navigator.getBattery().then(function(battery) {
            state.lowPowerMode = battery.level < 0.2 || battery.dischargingTime < 3600;
            
            if (state.lowPowerMode) {
                log('Low battery detected, enabling power saving mode', 'BATTERY');
                
                // Extend intervals to save battery
                config.NOW_PLAYING_INTERVAL *= 1.5;
                config.MOBILE_HEARTBEAT_INTERVAL *= 1.5;
                config.POSITION_SAVE_INTERVAL *= 2;
                
                showStatus('Low battery - enabled power saving mode', false, true);
            }
            
            // Listen for battery changes
            battery.addEventListener('levelchange', function() {
                const wasLowPower = state.lowPowerMode;
                state.lowPowerMode = battery.level < 0.2;
                
                if (wasLowPower !== state.lowPowerMode) {
                    log(`Battery mode changed: ${state.lowPowerMode ? 'Low' : 'Normal'} power`, 'BATTERY');
                    
                    if (state.lowPowerMode) {
                        showStatus('Low battery - reducing activity', false, true);
                    }
                }
            });
        }).catch(function(error) {
            log(`Battery API not available: ${error.message}`, 'BATTERY');
        });
    }
}

// Enhanced mobile initialization
function initMobileFeatures() {
    // Network adaptation
    adaptToNetworkQuality();
    
    // Battery status
    handleBatteryStatus();
    
    // Service worker registration (for better offline handling)
    if ('serviceWorker' in navigator) {
        navigator.serviceWorker.register('/sw.js').then(function(registration) {
            log('Service Worker registered successfully', 'SW');
        }).catch(function(error) {
            log(`Service Worker registration failed: ${error.message}`, 'SW');
        });
    }
    
    // Page visibility optimization
    let visibilityTimer;
    document.addEventListener('visibilitychange', function() {
        if (document.hidden) {
            // Reduce activity when page is hidden
            visibilityTimer = setTimeout(() => {
                if (state.isPlaying && document.hidden) {
                    log('Page hidden for extended time, reducing update frequency', 'VISIBILITY');
                    
                    // Pause non-critical timers
                    if (state.nowPlayingTimer) {
                        clearInterval(state.nowPlayingTimer);
                        state.nowPlayingTimer = setInterval(fetchNowPlaying, 60000); // 1 minute
                    }
                }
            }, 30000); // After 30 seconds of being hidden
        } else {
            // Restore normal activity when page becomes visible
            if (visibilityTimer) {
                clearTimeout(visibilityTimer);
                visibilityTimer = null;
            }
            
            if (state.isPlaying) {
                log('Page visible again, restoring normal activity', 'VISIBILITY');
                setupPlayingTimers(); // Restore normal timers
                sendHeartbeat(); // Immediate heartbeat
            }
        }
    });
}

// Performance monitoring
function startPerformanceMonitoring() {
    let lastMemoryCheck = 0;
    const memoryCheckInterval = 30000; // 30 seconds
    
    setInterval(() => {
        const now = Date.now();
        if (now - lastMemoryCheck > memoryCheckInterval) {
            lastMemoryCheck = now;
            
            // Check memory usage if available
            if (performance.memory) {
                const memory = performance.memory;
                const usedMB = Math.round(memory.usedJSHeapSize / 1048576);
                const limitMB = Math.round(memory.jsHeapSizeLimit / 1048576);
                
                if (usedMB > limitMB * 0.8) {
                    log(`High memory usage detected: ${usedMB}MB/${limitMB}MB`, 'PERFORMANCE', true);
                    
                    // Trigger garbage collection if possible
                    if (window.gc) {
                        window.gc();
                        log('Manual garbage collection triggered', 'PERFORMANCE');
                    }
                }
            }
            
            // Check for audio element leaks
            const audioElements = document.querySelectorAll('audio');
            if (audioElements.length > 3) {
                log(`Potential audio element leak detected: ${audioElements.length} elements`, 'PERFORMANCE', true);
            }
        }
    }, 10000); // Check every 10 seconds
}

// Enhanced error tracking
let errorHistory = [];
const MAX_ERROR_HISTORY = 10;

function trackError(error, context) {
    const errorRecord = {
        timestamp: Date.now(),
        error: error.message || error,
        context: context,
        userAgent: navigator.userAgent,
        url: window.location.href,
        position: getCurrentEstimatedPosition()
    };
    
    errorHistory.unshift(errorRecord);
    if (errorHistory.length > MAX_ERROR_HISTORY) {
        errorHistory = errorHistory.slice(0, MAX_ERROR_HISTORY);
    }
    
    // Log patterns
    const recentErrors = errorHistory.filter(e => Date.now() - e.timestamp < 60000);
    if (recentErrors.length > 3) {
        log(`High error frequency detected: ${recentErrors.length} errors in last minute`, 'ERROR', true);
        
        // Consider more drastic recovery measures
        if (recentErrors.length > 5 && state.isPlaying) {
            log('Too many recent errors, stopping playback', 'ERROR', true);
            showStatus('Too many errors - please refresh the page', true);
            stopAudio(true);
        }
    }
}

// Cleanup function
function cleanup() {
    log('Cleaning up player resources', 'CLEANUP');
    
    // Clear all timers
    const timers = [
        'positionSaveTimer',
        'heartbeatTimer', 
        'nowPlayingTimer',
        'connectionHealthTimer'
    ];
    
    timers.forEach(timer => {
        if (state[timer]) {
            clearInterval(state[timer]);
            state[timer] = null;
        }
    });
    
    // Clean up audio element
    if (state.audioElement) {
        try {
            state.audioElement.pause();
            state.audioElement.src = '';
            state.audioElement.load();
        } catch (e) {
            log(`Error during audio cleanup: ${e.message}`, 'CLEANUP');
        }
    }
    
    // Save final position
    savePositionToStorage();
    
    log('Player cleanup completed', 'CLEANUP');
}

// Enhanced initialization with mobile features
function initializeEnhancedPlayer() {
    log('Initializing enhanced mobile player features', 'INIT');
    
    // Initialize mobile-specific features
    if (state.isMobile) {
        initMobileFeatures();
    }
    
    // Start performance monitoring
    if (config.DEBUG_MODE) {
        startPerformanceMonitoring();
    }
    
    // Set up global error handling
    window.addEventListener('error', function(event) {
        trackError(event.error, 'global');
        log(`Global error: ${event.error.message}`, 'ERROR', true);
    });
    
    window.addEventListener('unhandledrejection', function(event) {
        trackError(event.reason, 'promise');
        log(`Unhandled promise rejection: ${event.reason}`, 'ERROR', true);
    });
    
    log('Enhanced player features initialized', 'INIT');
}

// Handle page unload
window.addEventListener('beforeunload', () => {
    log('Page unloading, saving state', 'LIFECYCLE');
    savePositionToStorage();
    cleanup();
});

// Handle page visibility for mobile battery optimization
document.addEventListener('visibilitychange', () => {
    if (document.hidden) {
        log('Page hidden - reducing activity', 'VISIBILITY');
        if (state.isPlaying) {
            savePositionToStorage();
        }
    } else {
        log('Page visible - resuming activity', 'VISIBILITY');
        if (state.isPlaying) {
            sendHeartbeat();
        }
    }
});

// Network change handling
window.addEventListener('online', () => {
    log('Network connection restored', 'NETWORK');
    if (state.isPlaying && state.audioElement && state.audioElement.paused) {
        showStatus('Connection restored - reconnecting...', false, true);
        setTimeout(() => {
            attemptReconnection('network restored');
        }, 1000);
    }
});

window.addEventListener('offline', () => {
    log('Network connection lost', 'NETWORK', true);
    showStatus('Network connection lost', true);
});

// Add missing connection ID extraction from response
async function extractConnectionId(response) {
    try {
        const connectionId = response.headers.get('X-Connection-ID');
        if (connectionId) {
            state.connectionId = connectionId;
            log(`Connection ID received: ${connectionId.substring(0, 8)}`, 'CONNECTION');
            return connectionId;
        }
    } catch (e) {
        log(`Could not extract connection ID: ${e.message}`, 'CONNECTION');
    }
    
    // Generate fallback ID if server doesn't provide one
    state.connectionId = 'client-' + Math.random().toString(36).substring(2, 15);
    return state.connectionId;
}

// Network status monitoring
function monitorNetworkStatus() {
    if (navigator.connection) {
        const updateNetworkInfo = () => {
            const connection = navigator.connection;
            const oldType = state.networkType;
            state.networkType = connection.effectiveType || 'unknown';
            
            if (oldType !== state.networkType) {
                log(`Network changed: ${oldType} -> ${state.networkType}`, 'NETWORK');
                adaptToNetworkQuality();
                
                if (state.isPlaying) {
                    showStatus(`Network: ${state.networkType.toUpperCase()}`, false, true);
                }
            }
        };
        
        navigator.connection.addEventListener('change', updateNetworkInfo);
        updateNetworkInfo(); // Initial check
    }
}

// Audio context management for mobile
function setupAudioContext() {
    if (!('AudioContext' in window) && !('webkitAudioContext' in window)) {
        log('AudioContext not supported', 'AUDIO');
        return null;
    }
    
    try {
        const AudioContextClass = window.AudioContext || window.webkitAudioContext;
        const audioContext = new AudioContextClass();
        
        if (audioContext.state === 'suspended') {
            log('AudioContext suspended, will resume on user interaction', 'AUDIO');
            
            const resumeAudioContext = () => {
                if (audioContext.state === 'suspended') {
                    audioContext.resume().then(() => {
                        log('AudioContext resumed successfully', 'AUDIO');
                    }).catch(e => {
                        log(`AudioContext resume failed: ${e.message}`, 'AUDIO', true);
                    });
                }
            };
            
            // Resume on any user interaction
            ['touchstart', 'touchend', 'mousedown', 'keydown'].forEach(eventType => {
                document.addEventListener(eventType, resumeAudioContext, { once: true });
            });
        }
        
        return audioContext;
    } catch (e) {
        log(`AudioContext creation failed: ${e.message}`, 'AUDIO', true);
        return null;
    }
}

// Enhanced mobile reconnection with backoff
function calculateReconnectionDelay(attempt) {
    const baseDelay = config.RECONNECT_MIN_DELAY;
    const maxDelay = config.RECONNECT_MAX_DELAY;
    
    // Exponential backoff with jitter
    let delay = Math.min(baseDelay * Math.pow(1.5, attempt - 1), maxDelay);
    
    // Network-based adjustments
    if (state.networkType === '2g' || state.networkType === 'slow-2g') {
        delay *= 2;
    } else if (state.networkType === '3g') {
        delay *= 1.5;
    }
    
    // Add jitter (±25%)
    const jitter = delay * 0.25 * (Math.random() - 0.5);
    delay += jitter;
    
    // Battery consideration
    if (state.lowPowerMode) {
        delay *= 1.5;
    }
    
    return Math.max(1000, Math.round(delay)); // Minimum 1 second
}

// Connection state management
function updateConnectionState(newState) {
    const oldState = state.connectionState || 'disconnected';
    state.connectionState = newState;
    
    if (oldState !== newState) {
        log(`Connection state: ${oldState} -> ${newState}`, 'CONNECTION');
        
        // Update UI based on connection state
        switch (newState) {
            case 'connecting':
                startBtn.disabled = true;
                startBtn.textContent = 'Connecting...';
                break;
            case 'connected':
                startBtn.disabled = false;
                startBtn.textContent = 'Disconnect';
                startBtn.dataset.connected = 'true';
                break;
            case 'disconnected':
                startBtn.disabled = false;
                startBtn.textContent = 'Connect';
                startBtn.dataset.connected = 'false';
                break;
            case 'reconnecting':
                startBtn.disabled = true;
                startBtn.textContent = 'Reconnecting...';
                break;
            case 'error':
                startBtn.disabled = false;
                startBtn.textContent = 'Retry';
                break;
        }
    }
}

// Media session API for mobile controls
function setupMediaSession() {
    if (!('mediaSession' in navigator)) {
        log('Media Session API not supported', 'MEDIA');
        return;
    }
    
    try {
        navigator.mediaSession.metadata = new MediaMetadata({
            title: state.currentTrack?.title || 'ChillOut Radio',
            artist: state.currentTrack?.artist || 'Unknown Artist',
            album: state.currentTrack?.album || 'Live Stream',
            artwork: [
                { src: '/static/icon-96.png', sizes: '96x96', type: 'image/png' },
                { src: '/static/icon-192.png', sizes: '192x192', type: 'image/png' },
                { src: '/static/icon-512.png', sizes: '512x512', type: 'image/png' }
            ]
        });
        
        // Set up action handlers
        navigator.mediaSession.setActionHandler('play', () => {
            if (!state.isPlaying) {
                startAudio();
            }
        });
        
        navigator.mediaSession.setActionHandler('pause', () => {
            if (state.isPlaying) {
                stopAudio();
            }
        });
        
        navigator.mediaSession.setActionHandler('stop', () => {
            if (state.isPlaying) {
                stopAudio();
            }
        });
        
        // Position information
        navigator.mediaSession.setPositionState({
            duration: state.currentTrack?.duration || 0,
            playbackRate: 1.0,
            position: getCurrentEstimatedPosition()
        });
        
        log('Media Session API configured', 'MEDIA');
    } catch (e) {
        log(`Media Session setup failed: ${e.message}`, 'MEDIA', true);
    }
}

// Update media session when track changes
function updateMediaSession() {
    if (!('mediaSession' in navigator) || !state.currentTrack) return;
    
    try {
        navigator.mediaSession.metadata = new MediaMetadata({
            title: state.currentTrack.title || 'ChillOut Radio',
            artist: state.currentTrack.artist || 'Unknown Artist',
            album: state.currentTrack.album || 'Live Stream',
            artwork: [
                { src: '/static/icon-96.png', sizes: '96x96', type: 'image/png' },
                { src: '/static/icon-192.png', sizes: '192x192', type: 'image/png' },
                { src: '/static/icon-512.png', sizes: '512x512', type: 'image/png' }
            ]
        });
        
        navigator.mediaSession.setPositionState({
            duration: state.currentTrack.duration || 0,
            playbackRate: 1.0,
            position: getCurrentEstimatedPosition()
        });
        
        log(`Media session updated: ${state.currentTrack.title}`, 'MEDIA');
    } catch (e) {
        log(`Media session update failed: ${e.message}`, 'MEDIA', true);
    }
}

// Wake lock management for mobile
let wakeLock = null;

async function requestWakeLock() {
    if (!('wakeLock' in navigator)) {
        log('Wake Lock API not supported', 'WAKE');
        return;
    }
    
    try {
        wakeLock = await navigator.wakeLock.request('screen');
        log('Wake lock acquired', 'WAKE');
        
        wakeLock.addEventListener('release', () => {
            log('Wake lock released', 'WAKE');
            wakeLock = null;
        });
        
    } catch (e) {
        log(`Wake lock failed: ${e.message}`, 'WAKE');
    }
}

async function releaseWakeLock() {
    if (wakeLock) {
        try {
            await wakeLock.release();
            wakeLock = null;
            log('Wake lock released manually', 'WAKE');
        } catch (e) {
            log(`Wake lock release failed: ${e.message}`, 'WAKE');
        }
    }
}

// Initialize the mobile-optimized player
function initPlayer() {
    log("Initializing mobile-optimized ChillOut Radio player");
    log(`Platform: ${state.isMobile ? 'Mobile' : 'Desktop'}, iOS: ${state.isIOS}, Android: ${state.isAndroid} (v${state.androidVersion}), Safari: ${state.isSafari}`);
    
    // Detect network conditions
    detectNetworkConditions();
    
    // Verify UI elements
    if (!startBtn || !muteBtn || !volumeControl || !statusMessage) {
        log("Critical UI elements missing!", 'ERROR', true);
        alert("Player initialization failed: UI elements not found");
        return;
    }
    
    // Set up event listeners
    setupEventListeners();
    
    // Platform-specific optimizations
    if (state.isAndroid) {
        setupAndroidOptimizations();
    } else if (state.isIOS) {
        setupIOSOptimizations();
    }
    
    // Load saved settings
    loadSavedSettings();
    loadPositionFromStorage();
    
    // Set up mobile-optimized timers
    setupMobileTimers();
    
    // Initial track info fetch
    fetchNowPlaying();
    
    // Set up background/foreground handling
    setupVisibilityHandling();
    
    // Initialize connection state
    updateConnectionState('disconnected');
    
    log('Mobile-optimized player initialized successfully');
    showStatus('Player ready - tap Connect to start streaming', false, false);
}

// Connection quality assessment
function assessConnectionQuality() {
    const metrics = {
        rtt: 0,
        downlink: 0,
        effectiveType: 'unknown'
    };
    
    if (navigator.connection) {
        const conn = navigator.connection;
        metrics.rtt = conn.rtt || 0;
        metrics.downlink = conn.downlink || 0;
        metrics.effectiveType = conn.effectiveType || 'unknown';
    }
    
    // Calculate quality score (0-100)
    let quality = 100;
    
    if (metrics.rtt > 0) {
        if (metrics.rtt > 1000) quality -= 40;
        else if (metrics.rtt > 500) quality -= 20;
        else if (metrics.rtt > 200) quality -= 10;
    }
    
    if (metrics.downlink > 0) {
        if (metrics.downlink < 0.5) quality -= 30;
        else if (metrics.downlink < 1) quality -= 15;
        else if (metrics.downlink < 2) quality -= 5;
    }
    
    switch (metrics.effectiveType) {
        case 'slow-2g':
            quality = Math.min(quality, 20);
            break;
        case '2g':
            quality = Math.min(quality, 40);
            break;
        case '3g':
            quality = Math.min(quality, 70);
            break;
    }
    
    return {
        score: Math.max(0, quality),
        metrics: metrics,
        rating: quality >= 80 ? 'excellent' : 
                quality >= 60 ? 'good' : 
                quality >= 40 ? 'fair' : 'poor'
    };
}

// Advanced error recovery
function handleAdvancedErrorRecovery(error, context) {
    trackError(error, context);
    
    const recentErrors = errorHistory.filter(e => Date.now() - e.timestamp < 300000); // 5 minutes
    const errorsByType = {};
    
    recentErrors.forEach(e => {
        const type = e.error.split(':')[0];
        errorsByType[type] = (errorsByType[type] || 0) + 1;
    });
    
    log(`Recent errors by type: ${JSON.stringify(errorsByType)}`, 'ERROR');
    
    // Implement progressive recovery strategies
    if (recentErrors.length > 5) {
        log('High error rate detected, implementing recovery strategy', 'ERROR', true);
        
        // Strategy 1: Clear all cached data
        try {
            localStorage.removeItem('radioPosition');
            localStorage.removeItem('radioVolume');
            localStorage.removeItem('radioMuted');
            log('Cleared cached data as recovery strategy', 'ERROR');
        } catch (e) {
            log(`Cache clear failed: ${e.message}`, 'ERROR');
        }
        
        // Strategy 2: Reset connection state
        state.consecutiveErrors = 0;
        state.reconnectAttempts = 0;
        state.positionDriftCorrection = 0;
        
        // Strategy 3: Suggest page refresh after many errors
        if (recentErrors.length > 10) {
            showStatus('Multiple errors detected - please refresh the page', true);
            return false; // Don't attempt auto-recovery
        }
    }
    
    return true; // Continue with auto-recovery
}

// Enhanced initialization function
function initializeEnhancedPlayer() {
    log('Initializing enhanced mobile player features', 'INIT');
    
    // Set up audio context
    setupAudioContext();
    
    // Set up media session
    setupMediaSession();
    
    // Monitor network status
    monitorNetworkStatus();
    
    // Initialize mobile-specific features
    if (state.isMobile) {
        initMobileFeatures();
        
        // Request wake lock when playing starts
        document.addEventListener('play', requestWakeLock);
        document.addEventListener('pause', releaseWakeLock);
    }
    
    // Start performance monitoring in debug mode
    if (config.DEBUG_MODE) {
        startPerformanceMonitoring();
        
        // Log connection quality periodically
        setInterval(() => {
            const quality = assessConnectionQuality();
            log(`Connection quality: ${quality.rating} (${quality.score}/100)`, 'QUALITY');
        }, 60000); // Every minute
    }
    
    // Set up global error handling
    window.addEventListener('error', function(event) {
        const shouldRecover = handleAdvancedErrorRecovery(event.error, 'global');
        if (!shouldRecover) {
            stopAudio(true);
        }
    });
    
    window.addEventListener('unhandledrejection', function(event) {
        const shouldRecover = handleAdvancedErrorRecovery(event.reason, 'promise');
        if (!shouldRecover) {
            stopAudio(true);
        }
    });
    
    // Connection state initialization
    updateConnectionState('disconnected');
    
    log('Enhanced player features initialized successfully', 'INIT');
}

// Final initialization wrapper
function finalizePlayerInitialization() {
    try {
        // Ensure all required elements are present
        const requiredElements = [
            'start-btn', 'mute-btn', 'volume', 'status-message',
            'listener-count', 'current-title', 'current-artist',
            'current-album', 'current-position', 'current-duration', 'progress-bar'
        ];
        
        const missing = requiredElements.filter(id => !document.getElementById(id));
        if (missing.length > 0) {
            throw new Error(`Missing required elements: ${missing.join(', ')}`);
        }
        
        // Initialize core player
        initPlayer();
        
        // Initialize enhanced features
        initializeEnhancedPlayer();
        
        // Final setup
        log('Player initialization completed successfully', 'INIT');
        showStatus('Player ready - tap Connect to start streaming', false, false);
        
    } catch (error) {
        log(`Player initialization failed: ${error.message}`, 'INIT', true);
        alert(`Player initialization failed: ${error.message}`);
    }
}

// Initialize when DOM is ready
document.addEventListener('DOMContentLoaded', finalizePlayerInitialization);d = state.isMuted;
        }
        
        muteBtn.textContent = state.isMuted ? 'Unmute' : 'Mute';
        
        try {
            localStorage.setItem('radioMuted', state.isMuted.toString());
        } catch (e) {
            // Ignore storage errors
        }
    });
    
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
}

// Load saved settings
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
        log(`Error loading settings: ${e.message}`, 'STORAGE');
    }
}

// Load position from localStorage
function loadPositionFromStorage() {
    try {
        const saved = localStorage.getItem('radioPosition');
        if (saved) {
            const data = JSON.parse(saved);
            const age = Date.now() - data.timestamp;
            
            // Mobile devices get longer position validity
            const maxAge = state.isMobile ? 45000 : 30000;
            
            if (age < maxAge) {
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

// Save position to localStorage (mobile-optimized)
function savePositionToStorage() {
    try {
        if (state.currentTrackId && state.isPlaying) {
            const currentPos = getCurrentEstimatedPosition();
            const positionData = {
                trackId: state.currentTrackId,
                position: currentPos,
                timestamp: Date.now(),
                platform: state.isAndroid ? 'android' : (state.isIOS ? 'ios' : 'mobile'),
                connectionId: state.connectionId
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

// Mobile-optimized audio element creation
function createAudioElement() {
    if (state.audioElement && !state.isCleaningUp) {
        log('Audio element already exists', 'AUDIO');
        return;
    }
    
    log(`Creating mobile-optimized audio element`, 'AUDIO');
    
    state.audioElement = new Audio();
    state.audioElement.controls = false;
    state.audioElement.volume = state.volume;
    state.audioElement.muted = state.isMuted;
    state.audioElement.crossOrigin = "anonymous";
    
    // Mobile-specific settings
    if (state.isMobile) {
        state.audioElement.preload = 'metadata'; // Less aggressive preloading for mobile
        state.audioElement.autoplay = false;
        
        // Mobile attributes
        if (state.audioElement.setAttribute) {
            state.audioElement.setAttribute('webkit-playsinline', 'true');
            state.audioElement.setAttribute('playsinline', 'true');
        }
        
        if (state.isIOS) {
            state.audioElement.playsInline = true;
        }
    } else {
        state.audioElement.preload = 'auto';
    }
    
    // Set up mobile-optimized audio event listeners
    setupMobileAudioListeners();
    
    log(`Mobile-optimized audio element created`, 'AUDIO');
}

// Start audio with mobile optimization
function startAudio() {
    log('Starting mobile-optimized audio playback', 'CONTROL');
    
    if (state.isPlaying || state.isReconnecting) {
        log('Already playing or reconnecting, ignoring start request', 'CONTROL');
        return;
    }
    
    // Update connection state
    updateConnectionState('connecting');
    showStatus('Connecting to stream...', false, false);
    
    // Reset state
    state.isPlaying = true;
    state.isReconnecting = false;
    state.reconnectAttempts = 0;
    state.trackChangeDetected = false;
    state.pendingPlay = false;
    state.positionDriftCorrection = 0;
    state.consecutiveErrors = 0;
    
    // Get fresh track info and position
    fetchNowPlaying().then(() => {
        log(`Starting playback with server position: ${state.serverPosition}s + ${state.serverPositionMs}ms`, 'CONTROL');
        
        // Initialize client position tracking
        state.clientStartTime = Date.now();
        state.clientPositionOffset = state.serverPosition;
        
        // Clean up and create new audio element
        cleanupAudioElement().then(() => {
            createAudioElement();
            
            if (state.isMobile) {
                startMobileDirectPlayback();
            } else {
                startDirectPlayback();
            }
            
            setupPlayingTimers();
        });
    }).catch((error) => {
        log(`Failed to fetch current position: ${error.message}`, 'CONTROL', true);
        
        // If fetch fails, still try to start with saved position
        const savedPos = loadPositionFromStorage();
        if (savedPos && savedPos.trackId === state.currentTrackId) {
            state.serverPosition = savedPos.position;
            log(`Using saved position: ${state.serverPosition}s`, 'CONTROL');
        } else {
            state.serverPosition = 0;
            log('No reliable position data, starting from beginning', 'CONTROL');
        }
        
        state.clientStartTime = Date.now();
        state.clientPositionOffset = state.serverPosition;
        
        cleanupAudioElement().then(() => {
            createAudioElement();
            
            if (state.isMobile) {
                startMobileDirectPlayback();
            } else {
                startDirectPlayback();
            }
            
            setupPlayingTimers();
        });
    });
}

// Stop audio with mobile cleanup
function stopAudio(isError = false) {
    log(`Stopping mobile audio playback${isError ? ' (due to error)' : ''}`, 'CONTROL');
    
    // Record disconnection for continuity
    if (isError && state.isPlaying) {
        state.disconnectionTime = Date.now();
        state.lastKnownPosition = getCurrentEstimatedPosition();
        log(`Recorded mobile disconnection at position ${state.lastKnownPosition.toFixed(1)}s`, 'MOBILE');
    } else {
        state.disconnectionTime = null;
    }
    
    state.isPlaying = false;
    state.isReconnecting = false;
    state.pendingPlay = false;
    
    // Update connection state
    updateConnectionState('disconnected');
    
    // Clear all timers
    const timers = ['nowPlayingTimer', 'connectionHealthTimer'];
    timers.forEach(timer => {
        if (state[timer]) {
            clearInterval(state[timer]);
            state[timer] = null;
        }
    });
    
    // Release wake lock
    releaseWakeLock();
    
    cleanupAudioElement().then(() => {
        log('Mobile audio cleanup completed', 'CONTROL');
    });
    
    if (!isError) {
        showStatus('Disconnected from audio stream');
    }
}

// Toggle connection
function toggleConnection() {
    const isConnected = startBtn.dataset.connected === 'true';
    
    if (isConnected || state.isPlaying) {
        log('User requested disconnect', 'CONTROL');
        stopAudio();
    } else {
        log('User requested connect', 'CONTROL');
        startAudio();
    }
}

// Mobile-optimized direct playback
function startMobileDirectPlayback() {
    if (!state.audioElement) {
        log('No audio element for mobile playback', 'MOBILE', true);
        updateConnectionState('error');
        return;
    }
    
    try {
        const timestamp = Date.now();
        const syncPosition = state.serverPosition;
        
        log(`Mobile streaming with position: ${syncPosition}s`, 'MOBILE');
        
        // Create mobile-optimized URL
        let streamUrl = `/direct-stream?t=${timestamp}&position=${syncPosition}`;
        
        // Add platform identification
        if (state.isAndroid) {
            streamUrl += '&platform=android';
        } else if (state.isIOS) {
            streamUrl += '&platform=ios';
        } else if (state.isMobile) {
            streamUrl += '&platform=mobile';
        }
        
        log(`Mobile stream URL: ${streamUrl}`, 'MOBILE');
        
        // Update client position tracking
        state.clientStartTime = Date.now();
        state.clientPositionOffset = syncPosition;
        state.disconnectionTime = null;
        
        // Set source
        state.audioElement.src = streamUrl;
        
        log('Starting mobile playback attempt', 'MOBILE');
        showStatus('Connecting to mobile stream...', false, false);
        
        // Mobile-specific playback with user interaction handling
        setTimeout(() => {
            if (state.audioElement && state.isPlaying && !state.isCleaningUp) {
                attemptPlayback();
            }
        }, state.isMobile ? 800 : 200);
        
    } catch (e) {
        log(`Mobile streaming setup error: ${e.message}`, 'MOBILE', true);
        showStatus(`Mobile streaming error: ${e.message}`, true);
        updateConnectionState('error');
        stopAudio(true);
    }
}

// Attempt playback (unified for all platforms)
function attemptPlayback() {
    if (!state.audioElement || state.isCleaningUp) {
        log('No audio element available for playback', 'AUDIO', true);
        return;
    }
    
    const playPromise = state.audioElement.play();
    if (playPromise !== undefined) {
        playPromise.then(() => {
            log(`Playback started successfully`, 'AUDIO');
            updateConnectionState('connected');
            showStatus('Stream connected');
            
            // Request wake lock for mobile
            if (state.isMobile) {
                requestWakeLock();
            }
            
        }).catch(e => {
            log(`Playback failed: ${e.message}`, 'AUDIO', true);
            handleMobilePlaybackFailure(e);
        });
    }
}

// Handle mobile playback failures
function handleMobilePlaybackFailure(error) {
    log(`Mobile playbook failure: ${error.name} - ${error.message}`, 'MOBILE', true);
    
    if (error.name === 'NotAllowedError') {
        showStatus('Please tap to enable audio playback', true, false);
        updateConnectionState('error');
        startBtn.onclick = function() {
            state.userHasInteracted = true;
            attemptPlayback();
        };
    } else {
        showStatus(`Mobile playback failed - ${error.message}`, true);
        updateConnectionState('error');
        
        setTimeout(() => {
            if (state.isPlaying && !state.isReconnecting) {
                attemptReconnection('mobile playback failure');
            }
        }, state.isMobile ? 4000 : 2000);
    }
}

// Start direct HTTP streaming (for desktop)
function startDirectPlayback() {
    if (!state.audioElement) {
        log('No audio element available for playback', 'PLAYBACK', true);
        return;
    }
    
    try {
        const timestamp = Date.now();
        let syncPosition = state.serverPosition;
        
        // Desktop position continuity logic
        if (state.disconnectionTime && (timestamp - state.disconnectionTime) < state.maxReconnectGap) {
            const timeSinceDisconnect = (timestamp - state.disconnectionTime) / 1000;
            const estimatedPosition = state.lastKnownPosition + timeSinceDisconnect;
            
            if (Math.abs(estimatedPosition - state.serverPosition) < config.POSITION_SYNC_TOLERANCE) {
                syncPosition = Math.floor(estimatedPosition);
                log(`Using continuity position: ${syncPosition}s`, 'SYNC');
            }
        }
        
        let streamUrl = `/direct-stream?t=${timestamp}&position=${syncPosition}`;
        
        if (state.isSafari) {
            streamUrl += '&platform=safari';
        }
        
        log(`Desktop stream URL: ${streamUrl}`, 'PLAYBACK');
        
        state.clientStartTime = Date.now();
        state.clientPositionOffset = syncPosition;
        state.disconnectionTime = null;
        
        if (state.audioElement && !state.isCleaningUp) {
            state.audioElement.src = streamUrl;
        }
        
        setTimeout(() => {
            attemptPlayback();
        }, 200);
        
    } catch (e) {
        log(`Direct streaming error: ${e.message}`, 'AUDIO', true);
        showStatus(`Streaming error: ${e.message}`, true);
        stopAudio(true);
    }
}

// Connection state management
function updateConnectionState(newState) {
    const oldState = state.connectionState || 'disconnected';
    state.connectionState = newState;
    
    if (oldState !== newState) {
        log(`Connection state: ${oldState} -> ${newState}`, 'CONNECTION');
        
        // Update UI based on connection state
        switch (newState) {
            case 'connecting':
                startBtn.disabled = true;
                startBtn.textContent = 'Connecting...';
                startBtn.dataset.connected = 'false';
                break;
            case 'connected':
                startBtn.disabled = false;
                startBtn.textContent = 'Disconnect';
                startBtn.dataset.connected = 'true';
                break;
            case 'disconnected':
                startBtn.disabled = false;
                startBtn.textContent = 'Connect';
                startBtn.dataset.connected = 'false';
                break;
            case 'reconnecting':
                startBtn.disabled = true;
                startBtn.textContent = 'Reconnecting...';
                break;
            case 'error':
                startBtn.disabled = false;
                startBtn.textContent = 'Retry';
                startBtn.dataset.connected = 'false';
                break;
        }
    }
}