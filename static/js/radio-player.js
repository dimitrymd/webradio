// static/js/radio-player.js - Part 1: Configuration and State Setup

// Radio configuration - simplified for synchronized streaming
const RADIO_CONFIG = {
    NOW_PLAYING_INTERVAL: 10000,        // Check every 10 seconds
    CONNECTION_CHECK_INTERVAL: 8000,    // Check connection every 8 seconds
    RECONNECT_ATTEMPTS: 5,              
    DEBUG_MODE: true,
    
    // Radio-specific settings
    RADIO_MODE: true,                   // Always true for radio streaming
    SEEKING_ENABLED: false,             // No seeking in radio mode
    SYNCHRONIZED_PLAYBACK: true,        // All clients synchronized
    
    // Simplified error handling for radio
    MAX_ERROR_FREQUENCY: 8000,          
    CLEANUP_DELAY: 500,                 
    RECONNECT_MIN_DELAY: 2000,          
    RECONNECT_MAX_DELAY: 15000,         
    
    // Track transition
    TRACK_CHANGE_GRACE_PERIOD: 3000,    
    
    // Mobile-specific timeouts
    MOBILE_BUFFER_TIMEOUT: 12000,       
    MOBILE_HEARTBEAT_INTERVAL: 15000,   
    STALE_CONNECTION_TIMEOUT: 30000,    
};

// Radio state management
const radioState = {
    // Audio and connection
    audioElement: null,
    cleanupTimeout: null,
    isCleaningUp: false,
    userHasInteracted: false,
    connectionId: null,
    lastHeartbeat: 0,
    connectionState: 'disconnected',
    
    // Connection status
    isPlaying: false,
    isMuted: false,
    volume: 0.7,
    lastTrackInfoTime: Date.now(),
    lastErrorTime: 0,
    reconnectAttempts: 0,
    isReconnecting: false,
    consecutiveErrors: 0,
    
    // Radio track info (no client position tracking needed)
    currentTrackId: null,
    currentTrack: null,
    radioPosition: 0,           // Server radio position
    radioPositionMs: 0,         // Server radio position (milliseconds)
    trackChangeDetected: false,
    trackChangeTime: 0,
    
    // Simplified radio state (no client-side position estimation)
    serverTimestamp: 0,
    lastRadioUpdate: 0,
    
    // Timers
    nowPlayingTimer: null,
    connectionHealthTimer: null,
    heartbeatTimer: null,
    
    // Platform detection
    isIOS: /iPad|iPhone|iPod/.test(navigator.userAgent) && !window.MSStream,
    isSafari: /^((?!chrome|android).)*safari/i.test(navigator.userAgent),
    isMobile: /Mobi|Android/i.test(navigator.userAgent),
    isAndroid: /Android/i.test(navigator.userAgent),
    androidVersion: navigator.userAgent.match(/Android (\d+)/)?.[1] || 'unknown',
    
    // Mobile-specific state
    backgroundTime: 0,
    networkType: 'unknown',
    lowPowerMode: false,
    
    // iOS-specific
    iosPlaybackUnlocked: false,
    pendingPlay: false,
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

// static/js/radio-player.js - Part 2: Initialization and Setup Functions

// Initialize the radio player
function initRadioPlayer() {
    log("Initializing ChillOut Radio - Live Radio Stream", 'RADIO');
    log(`Platform: ${radioState.isMobile ? 'Mobile' : 'Desktop'}, iOS: ${radioState.isIOS}, Android: ${radioState.isAndroid} (v${radioState.androidVersion}), Safari: ${radioState.isSafari}`, 'RADIO');
    
    // Detect network conditions
    detectNetworkConditions();
    
    // Verify UI elements
    if (!startBtn || !muteBtn || !volumeControl || !statusMessage) {
        log("Critical UI elements missing!", 'ERROR', true);
        alert("Radio player initialization failed: UI elements not found");
        return;
    }
    
    // Set up event listeners
    setupEventListeners();
    
    // Platform-specific optimizations
    if (radioState.isAndroid) {
        setupAndroidOptimizations();
    } else if (radioState.isIOS) {
        setupIOSOptimizations();
    }
    
    // Load saved settings
    loadSavedSettings();
    
    // Set up radio timers
    setupRadioTimers();
    
    // Initial track info fetch
    fetchNowPlaying();
    
    // Set up background/foreground handling
    setupVisibilityHandling();
    
    log('Radio player initialized successfully', 'RADIO');
    showStatus('📻 Radio ready - tap Connect to tune in to the live stream', false, false);
}

// Detect network conditions
function detectNetworkConditions() {
    if (navigator.connection) {
        const connection = navigator.connection;
        radioState.networkType = connection.effectiveType || 'unknown';
        
        log(`Network: ${radioState.networkType}, downlink: ${connection.downlink || 'unknown'} Mbps`, 'NETWORK');
        
        // Adjust timeouts for radio streaming
        if (radioState.networkType === '2g' || radioState.networkType === 'slow-2g') {
            RADIO_CONFIG.MOBILE_BUFFER_TIMEOUT = 20000;
            RADIO_CONFIG.NOW_PLAYING_INTERVAL = 15000;
            log("Slow connection detected, adjusting radio streaming settings", 'NETWORK');
        }
        
        // Listen for connection changes
        connection.addEventListener('change', () => {
            const newType = connection.effectiveType;
            if (newType !== radioState.networkType) {
                log(`Network changed: ${radioState.networkType} -> ${newType}`, 'NETWORK');
                radioState.networkType = newType;
                
                if (radioState.isPlaying && (newType === '2g' || newType === 'slow-2g')) {
                    showStatus('Slow connection - radio quality may vary', false, true);
                }
            }
        });
    }
}

// Setup radio timers
function setupRadioTimers() {
    // Now playing timer
    if (radioState.nowPlayingTimer) clearInterval(radioState.nowPlayingTimer);
    radioState.nowPlayingTimer = setInterval(fetchNowPlaying, RADIO_CONFIG.NOW_PLAYING_INTERVAL);
    
    // Heartbeat timer
    if (radioState.heartbeatTimer) clearInterval(radioState.heartbeatTimer);
    radioState.heartbeatTimer = setInterval(sendHeartbeat, RADIO_CONFIG.MOBILE_HEARTBEAT_INTERVAL);
    
    log('Radio timers configured', 'RADIO');
}

// Setup visibility handling for radio
function setupVisibilityHandling() {
    document.addEventListener('visibilitychange', function() {
        if (document.hidden) {
            radioState.backgroundTime = Date.now();
            log('Radio app went to background', 'RADIO');
            
            if (radioState.isPlaying) {
                // Reduce timer frequency to save battery
                if (radioState.nowPlayingTimer) {
                    clearInterval(radioState.nowPlayingTimer);
                    radioState.nowPlayingTimer = setInterval(fetchNowPlaying, 30000); // 30 seconds in background
                }
            }
        } else {
            if (radioState.backgroundTime > 0) {
                const backgroundDuration = Date.now() - radioState.backgroundTime;
                log(`Radio app returned to foreground after ${Math.round(backgroundDuration/1000)}s`, 'RADIO');
                
                if (radioState.isPlaying) {
                    // Check if audio is still playing
                    setTimeout(() => {
                        if (radioState.audioElement && radioState.audioElement.paused && radioState.isPlaying) {
                            log('Radio audio paused during background, attempting recovery', 'RADIO');
                            attemptReconnection('background recovery');
                        } else {
                            // Restore normal timer frequency
                            setupPlayingTimers();
                            fetchNowPlaying();
                        }
                    }, 1000);
                }
            }
        }
    });
}

// Android optimizations for radio
function setupAndroidOptimizations() {
    log(`Setting up Android radio optimizations for version ${radioState.androidVersion}`, 'ANDROID');
    
    // Android wake lock for radio listening
    if ('wakeLock' in navigator) {
        navigator.wakeLock.request('screen').catch(err => {
            log(`Android wake lock failed: ${err.message}`, 'ANDROID');
        });
    }
    
    // Handle Android audio context for radio
    if ('AudioContext' in window || 'webkitAudioContext' in window) {
        try {
            const AudioContextClass = window.AudioContext || window.webkitAudioContext;
            const audioContext = new AudioContextClass();
            if (audioContext.state === 'suspended') {
                document.addEventListener('touchstart', () => {
                    audioContext.resume().then(() => {
                        log('Android AudioContext resumed for radio', 'ANDROID');
                    });
                }, { once: true });
            }
        } catch (e) {
            log(`Android AudioContext setup failed: ${e.message}`, 'ANDROID');
        }
    }
}

// iOS optimizations for radio
function setupIOSOptimizations() {
    log("Setting up iOS radio optimizations", 'IOS');
    
    // iOS wake lock for radio
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

// static/js/radio-player.js - Part 3: Event Listeners and Settings

// Set up event listeners
function setupEventListeners() {
    startBtn.addEventListener('click', function(e) {
        e.preventDefault();
        radioState.userHasInteracted = true;
        toggleRadioConnection();
    });
    
    muteBtn.addEventListener('click', function(e) {
        e.preventDefault();
        radioState.userHasInteracted = true;
        
        radioState.isMuted = !radioState.isMuted;
        
        if (radioState.audioElement && !radioState.isCleaningUp) {
            radioState.audioElement.muted = radioState.isMuted;
        }
        
        muteBtn.textContent = radioState.isMuted ? '🔇 Unmute' : '🔊 Mute';
        
        try {
            localStorage.setItem('radioMuted', radioState.isMuted.toString());
        } catch (e) {
            // Ignore storage errors
        }
    });
    
    volumeControl.addEventListener('input', function(e) {
        radioState.userHasInteracted = true;
        radioState.volume = this.value;
        
        if (radioState.audioElement && !radioState.isCleaningUp) {
            radioState.audioElement.volume = radioState.volume;
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
            radioState.volume = parseFloat(savedVolume);
        }
        
        const savedMuted = localStorage.getItem('radioMuted');
        if (savedMuted !== null) {
            radioState.isMuted = savedMuted === 'true';
            muteBtn.textContent = radioState.isMuted ? '🔇 Unmute' : '🔊 Mute';
        }
    } catch (e) {
        log(`Error loading settings: ${e.message}`, 'STORAGE');
    }
}

// Send heartbeat for radio connection
async function sendHeartbeat() {
    if (!radioState.isPlaying || !radioState.connectionId) return;
    
    try {
        const response = await fetch(`/api/heartbeat?connection_id=${radioState.connectionId}`, {
            method: 'GET',
            headers: {
                'Cache-Control': 'no-cache'
            }
        });
        
        if (response.ok) {
            radioState.lastHeartbeat = Date.now();
            
            const data = await response.json();
            if (data.active_listeners !== undefined) {
                listenerCount.innerHTML = `<span class="radio-live">LIVE</span> • Listeners: ${data.active_listeners}`;
            }
            
            // Update radio position from heartbeat
            if (data.radio_position !== undefined) {
                radioState.radioPosition = data.radio_position;
                radioState.radioPositionMs = data.radio_position_ms || 0;
                radioState.lastRadioUpdate = Date.now();
            }
        }
    } catch (error) {
        log(`Radio heartbeat failed: ${error.message}`, 'RADIO');
    }
}

// Toggle radio connection
function toggleRadioConnection() {
    const isConnected = startBtn.dataset.connected === 'true';
    
    if (isConnected) {
        log('User requested radio disconnect', 'RADIO');
        stopRadio();
    } else {
        log('User requested radio connect', 'RADIO');
        startRadio();
    }
}

// static/js/radio-player.js - Part 4: Audio Element Creation and Management

// Create radio audio element
function createRadioAudioElement() {
    if (radioState.audioElement && !radioState.isCleaningUp) {
        log('Radio audio element already exists', 'RADIO');
        return;
    }
    
    log(`Creating radio audio element`, 'RADIO');
    
    radioState.audioElement = new Audio();
    radioState.audioElement.controls = false;
    radioState.audioElement.volume = radioState.volume;
    radioState.audioElement.muted = radioState.isMuted;
    radioState.audioElement.crossOrigin = "anonymous";
    
    // Radio-specific settings
    if (radioState.isMobile) {
        radioState.audioElement.preload = 'auto'; // Auto preload for radio
        radioState.audioElement.autoplay = false;
        
        if (radioState.audioElement.setAttribute) {
            radioState.audioElement.setAttribute('webkit-playsinline', 'true');
            radioState.audioElement.setAttribute('playsinline', 'true');
        }
        
        if (radioState.isIOS) {
            radioState.audioElement.playsInline = true;
        }
    } else {
        radioState.audioElement.preload = 'auto';
    }
    
    // Set up radio audio event listeners
    setupRadioAudioListeners();
    
    log(`Radio audio element created`, 'RADIO');
}

// Setup radio audio event listeners
function setupRadioAudioListeners() {
    if (!radioState.audioElement) return;
    
    radioState.audioElement.addEventListener('playing', () => {
        log('Radio playing', 'RADIO');
        showStatus('📻 Tuned in to ChillOut Radio');
        radioState.trackChangeDetected = false;
        radioState.pendingPlay = false;
        radioState.consecutiveErrors = 0;
        
        // Send heartbeat to confirm radio connection
        if (radioState.connectionId) {
            sendHeartbeat();
        }
    });
    
    radioState.audioElement.addEventListener('waiting', () => {
        log('Radio buffering', 'RADIO');
        showStatus('📻 Buffering radio stream...', false, false);
    });
    
    radioState.audioElement.addEventListener('stalled', () => {
        log('Radio stalled', 'RADIO');
        showStatus('📻 Radio signal weak - buffering', true, false);
        
        if (!radioState.isReconnecting && !radioState.trackChangeDetected) {
            const stalledTimeout = radioState.isMobile ? RADIO_CONFIG.MOBILE_BUFFER_TIMEOUT : 5000;
            setTimeout(() => {
                if (radioState.isPlaying && !radioState.isReconnecting && radioState.audioElement && radioState.audioElement.readyState < 3) {
                    log('Radio still stalled after timeout, attempting reconnection', 'RADIO');
                    attemptReconnection('radio stalled');
                }
            }, stalledTimeout);
        }
    });
    
    radioState.audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        const errorMsg = getErrorMessage(e.target.error);
        
        radioState.consecutiveErrors++;
        log(`Radio error: ${errorMsg} (code ${errorCode}, consecutive: ${radioState.consecutiveErrors})`, 'RADIO', true);
        
        if (radioState.isPlaying && !radioState.isCleaningUp) {
            const now = Date.now();
            
            if (now - radioState.lastErrorTime > RADIO_CONFIG.MAX_ERROR_FREQUENCY) {
                radioState.lastErrorTime = now;
                handleRadioError(errorCode, errorMsg);
            }
        }
    });
    
    radioState.audioElement.addEventListener('ended', () => {
        log('Radio stream ended', 'RADIO');
        
        if (radioState.isPlaying && !radioState.isReconnecting) {
            if (radioState.trackChangeDetected) {
                log('Radio stream ended during track change, reconnecting', 'RADIO');
            } else {
                log('Radio stream ended unexpectedly, attempting to recover', 'RADIO', true);
            }
            
            showStatus('📻 Track ended - tuning to next song', false, false);
            attemptReconnection('track ended');
        }
    });
    
    // Radio progress monitoring (simplified - just for display)
    radioState.audioElement.addEventListener('timeupdate', () => {
        if (radioState.audioElement && !radioState.isCleaningUp && radioState.currentTrack && radioState.currentTrack.duration) {
            // For radio, we display the server position, not client position
            updateProgressBar(radioState.radioPosition, radioState.currentTrack.duration);
        }
    });
}

// Enhanced cleanup for radio
function cleanupAudioElement() {
    return new Promise((resolve) => {
        if (radioState.cleanupTimeout) {
            clearTimeout(radioState.cleanupTimeout);
            radioState.cleanupTimeout = null;
        }
        
        if (!radioState.audioElement) {
            resolve();
            return;
        }
        
        log('Cleaning up radio audio element', 'RADIO');
        radioState.isCleaningUp = true;
        
        const elementToCleanup = radioState.audioElement;
        radioState.audioElement = null;
        
        try {
            elementToCleanup.pause();
        } catch (e) {
            log(`Error pausing during cleanup: ${e.message}`, 'RADIO');
        }
        
        try {
            elementToCleanup.src = '';
            elementToCleanup.load();
        } catch (e) {
            log(`Error clearing source during cleanup: ${e.message}`, 'RADIO');
        }
        
        const cleanupDelay = radioState.isMobile ? RADIO_CONFIG.CLEANUP_DELAY * 2 : RADIO_CONFIG.CLEANUP_DELAY;
        
        radioState.cleanupTimeout = setTimeout(() => {
            try {
                if (elementToCleanup.parentNode) {
                    elementToCleanup.remove();
                }
            } catch (e) {
                log(`Error removing element during cleanup: ${e.message}`, 'RADIO');
            }
            
            radioState.isCleaningUp = false;
            radioState.cleanupTimeout = null;
            resolve();
        }, cleanupDelay);
    });
}

// static/js/radio-player.js - Part 5: Radio Streaming Logic

// Start radio streaming
function startRadio() {
    log('Tuning in to ChillOut Radio', 'RADIO');
    
    if (radioState.isPlaying || radioState.isReconnecting) {
        log('Already tuned in or reconnecting, ignoring start request', 'RADIO');
        return;
    }
    
    startBtn.disabled = true;
    showStatus('📻 Tuning in to radio stream...', false, false);
    
    // Reset state
    radioState.isPlaying = true;
    radioState.isReconnecting = false;
    radioState.reconnectAttempts = 0;
    radioState.trackChangeDetected = false;
    radioState.pendingPlay = false;
    radioState.consecutiveErrors = 0;
    
    // Get current radio info
    fetchNowPlaying().then(() => {
        log(`Tuning to radio at position: ${radioState.radioPosition}s + ${radioState.radioPositionMs}ms`, 'RADIO');
        
        // Clean up and create new audio element
        cleanupAudioElement().then(() => {
            createRadioAudioElement();
            startRadioStream();
            setupPlayingTimers();
        });
    }).catch(() => {
        // If fetch fails, still try to connect to radio
        log('Failed to fetch current radio info, connecting anyway', 'RADIO');
        
        cleanupAudioElement().then(() => {
            createRadioAudioElement();
            startRadioStream();
            setupPlayingTimers();
        });
    });
}

// Start radio stream (no position parameter - always current time)
function startRadioStream() {
    if (!radioState.audioElement) {
        log('No audio element for radio streaming', 'RADIO', true);
        return;
    }
    
    try {
        const timestamp = Date.now();
        
        log(`Starting radio stream`, 'RADIO');
        
        // Create radio stream URL (no position parameter - server determines current time)
        let streamUrl = `/direct-stream?t=${timestamp}`;
        
        // Add platform identification
        if (radioState.isAndroid) {
            streamUrl += '&platform=android';
        } else if (radioState.isIOS) {
            streamUrl += '&platform=ios';
        } else if (radioState.isMobile) {
            streamUrl += '&platform=mobile';
        }
        
        log(`Radio stream URL: ${streamUrl}`, 'RADIO');
        
        // Set source
        radioState.audioElement.src = streamUrl;
        
        log('Starting radio playback attempt', 'RADIO');
        showStatus('📻 Connecting to radio stream...', false, false);
        
        // Radio-specific playback
        setTimeout(() => {
            if (radioState.audioElement && radioState.isPlaying && !radioState.isCleaningUp) {
                const playPromise = radioState.audioElement.play();
                if (playPromise !== undefined) {
                    playPromise.then(() => {
                        log(`Radio playback started successfully`, 'RADIO');
                        showStatus('📻 Tuned in to ChillOut Radio');
                        startBtn.textContent = '📻 Disconnect';
                        startBtn.disabled = false;
                        startBtn.dataset.connected = 'true';
                        
                    }).catch(e => {
                        log(`Radio playback failed: ${e.message}`, 'RADIO', true);
                        handleRadioPlaybackFailure(e);
                    });
                }
            }
        }, radioState.isMobile ? 800 : 200);
        
    } catch (e) {
        log(`Radio streaming setup error: ${e.message}`, 'RADIO', true);
        showStatus(`📻 Radio streaming error: ${e.message}`, true);
        stopRadio(true);
    }
}

// Stop radio with cleanup
function stopRadio(isError = false) {
    log(`Stopping radio playback${isError ? ' (due to error)' : ''}`, 'RADIO');
    
    radioState.isPlaying = false;
    radioState.isReconnecting = false;
    radioState.pendingPlay = false;
    
    // Clear all timers
    if (radioState.nowPlayingTimer) {
        clearInterval(radioState.nowPlayingTimer);
        radioState.nowPlayingTimer = null;
    }
    
    if (radioState.connectionHealthTimer) {
        clearInterval(radioState.connectionHealthTimer);
        radioState.connectionHealthTimer = null;
    }
    
    cleanupAudioElement().then(() => {
        log('Radio audio cleanup completed', 'RADIO');
    });
    
    if (!isError) {
        showStatus('📻 Disconnected from radio stream');
    }
    
    // Reset UI
    startBtn.textContent = '📻 Tune In';
    startBtn.disabled = false;
    startBtn.dataset.connected = 'false';
    startBtn.onclick = toggleRadioConnection;
}

// Setup timers for active radio playback
function setupPlayingTimers() {
    if (radioState.nowPlayingTimer) {
        clearInterval(radioState.nowPlayingTimer);
    }
    
    if (radioState.connectionHealthTimer) {
        clearInterval(radioState.connectionHealthTimer);
    }
    
    // Use radio-optimized intervals
    const nowPlayingInterval = radioState.isMobile ? RADIO_CONFIG.NOW_PLAYING_INTERVAL : 8000;
    const healthCheckInterval = radioState.isMobile ? RADIO_CONFIG.CONNECTION_CHECK_INTERVAL : 5000;
    
    radioState.nowPlayingTimer = setInterval(fetchNowPlaying, nowPlayingInterval);
    radioState.connectionHealthTimer = setInterval(checkRadioConnectionHealth, healthCheckInterval);
    
    log(`Radio timers set up: nowPlaying=${nowPlayingInterval}ms, health=${healthCheckInterval}ms`, 'RADIO');
}

// static/js/radio-player.js - Part 6: Error Handling and Reconnection

// Handle radio-specific errors
function handleRadioError(errorCode, errorMsg) {
    log(`Radio error handler: code ${errorCode}, message: ${errorMsg}`, 'RADIO', true);
    
    let reconnectDelay = RADIO_CONFIG.RECONNECT_MIN_DELAY;
    
    if (radioState.consecutiveErrors > 3) {
        reconnectDelay = Math.min(RADIO_CONFIG.RECONNECT_MAX_DELAY, reconnectDelay * radioState.consecutiveErrors);
        showStatus(`📻 Radio signal issues - waiting ${Math.round(reconnectDelay/1000)}s before retry`, true, false);
    } else if (errorCode === 4) { // MEDIA_ERR_SRC_NOT_SUPPORTED
        showStatus('📻 Radio format issue - getting fresh signal...', true, false);
        reconnectDelay = radioState.isMobile ? 3000 : 2000;
    } else if (errorCode === 2) { // MEDIA_ERR_NETWORK
        showStatus('📻 Network error - reconnecting to radio...', true, false);
        reconnectDelay = radioState.networkType === '2g' ? 5000 : (radioState.isMobile ? 3000 : 2000);
    } else {
        showStatus('📻 Radio error - will reconnect', true, false);
        reconnectDelay = radioState.isMobile ? 3000 : 2000;
    }
    
    setTimeout(() => {
        if (radioState.isPlaying && !radioState.isReconnecting) {
            attemptReconnection(`radio error code ${errorCode}`);
        }
    }, reconnectDelay);
}

// Handle radio playback failures
function handleRadioPlaybackFailure(error) {
    log(`Radio playback failure: ${error.name} - ${error.message}`, 'RADIO', true);
    
    if (error.name === 'NotAllowedError') {
        showStatus('📻 Please tap to enable radio audio playback', true, false);
        startBtn.disabled = false;
        startBtn.textContent = '🔊 Enable Audio';
        startBtn.onclick = function() {
            radioState.userHasInteracted = true;
            startRadioStream();
        };
    } else {
        showStatus(`📻 Radio playback failed - ${error.message}`, true);
        startBtn.disabled = false;
        
        setTimeout(() => {
            if (radioState.isPlaying && !radioState.isReconnecting) {
                attemptReconnection('radio playback failure');
            }
        }, radioState.isMobile ? 4000 : 2000);
    }
}

// Radio reconnection
function attemptReconnection(reason = 'unknown') {
    if (radioState.isReconnecting) {
        log(`Radio reconnection already in progress, ignoring request (reason: ${reason})`, 'RADIO');
        return;
    }
    
    if (!radioState.isPlaying) {
        log(`Not playing radio, ignoring reconnection request (reason: ${reason})`, 'RADIO');
        return;
    }
    
    if (radioState.reconnectAttempts >= RADIO_CONFIG.RECONNECT_ATTEMPTS) {
        log(`Maximum radio reconnection attempts (${RADIO_CONFIG.RECONNECT_ATTEMPTS}) reached`, 'RADIO', true);
        showStatus('📻 Could not reconnect to radio. Please try again later.', true);
        stopRadio(true);
        return;
    }
    
    radioState.isReconnecting = true;
    radioState.reconnectAttempts++;
    
    // Radio-friendly exponential backoff
    const baseDelay = Math.min(
        RADIO_CONFIG.RECONNECT_MIN_DELAY * Math.pow(1.3, radioState.reconnectAttempts - 1), 
        RADIO_CONFIG.RECONNECT_MAX_DELAY
    );
    
    let networkMultiplier = 1;
    if (radioState.networkType === '2g' || radioState.networkType === 'slow-2g') {
        networkMultiplier = 2;
    } else if (radioState.networkType === '3g') {
        networkMultiplier = 1.5;
    }
    
    const delay = (baseDelay * networkMultiplier) + (Math.random() * 1000);
    
    log(`Radio reconnection attempt ${radioState.reconnectAttempts}/${RADIO_CONFIG.RECONNECT_ATTEMPTS} in ${Math.round(delay/1000)}s (reason: ${reason})`, 'RADIO');
    showStatus(`📻 Reconnecting to radio (${radioState.reconnectAttempts}/${RADIO_CONFIG.RECONNECT_ATTEMPTS})...`, true, false);
    
    cleanupAudioElement().then(() => {
        setTimeout(() => {
            if (!radioState.isPlaying) {
                radioState.isReconnecting = false;
                return;
            }
            
            log(`Executing radio reconnection attempt ${radioState.reconnectAttempts}`, 'RADIO');
            
            createRadioAudioElement();
            
            fetchNowPlaying().then(() => {
                if (radioState.isPlaying && radioState.audioElement) {
                    startRadioStream();
                }
                
                setTimeout(() => {
                    radioState.isReconnecting = false;
                }, 3000);
            }).catch(() => {
                if (radioState.isPlaying && radioState.audioElement) {
                    startRadioStream();
                }
                radioState.isReconnecting = false;
            });
        }, delay);
    });
}

// Radio connection health check
function checkRadioConnectionHealth() {
    if (!radioState.isPlaying || radioState.isReconnecting) return;
    
    const now = Date.now();
    const timeSinceLastTrackInfo = (now - radioState.lastTrackInfoTime) / 1000;
    const timeSinceLastHeartbeat = (now - radioState.lastHeartbeat) / 1000;
    
    // Check if we need fresh track info
    if (timeSinceLastTrackInfo > RADIO_CONFIG.NOW_PLAYING_INTERVAL / 1000) {
        fetchNowPlaying();
    }
    
    // Check if heartbeat is too old
    if (timeSinceLastHeartbeat > RADIO_CONFIG.MOBILE_HEARTBEAT_INTERVAL / 1000 * 2) {
        sendHeartbeat();
    }
    
    if (radioState.audioElement && !radioState.isCleaningUp) {
        // Radio-specific health checks
        if (radioState.audioElement.paused && radioState.isPlaying && !radioState.trackChangeDetected) {
            log('Radio: Audio is paused unexpectedly', 'RADIO', true);
            
            const playPromise = radioState.audioElement.play();
            if (playPromise !== undefined) {
                playPromise.then(() => {
                    log('Radio: Successfully resumed paused audio', 'RADIO');
                }).catch(e => {
                    log(`Radio: Resume failed, will reconnect: ${e.message}`, 'RADIO');
                    attemptReconnection('radio unexpected pause');
                });
            }
        }
        
        if (radioState.audioElement.networkState === HTMLMediaElement.NETWORK_NO_SOURCE) {
            log('Radio: Audio has no source', 'RADIO', true);
            attemptReconnection('radio no source');
        }
    }
}

// static/js/radio-player.js - Part 7: API Communication and Track Info

// Fetch now playing with radio focus
async function fetchNowPlaying() {
    try {
        log("Fetching radio now playing information", 'RADIO');
        
        let apiUrl = '/api/now-playing';
        if (radioState.isMobile) {
            apiUrl += '?mobile_client=true';
        }
        
        const response = await fetch(apiUrl, {
            headers: {
                'Cache-Control': 'no-cache'
            }
        });
        
        if (!response.ok) {
            log(`Radio now playing API error: ${response.status}`, 'RADIO', true);
            return null;
        }
        
        const data = await response.json();
        updateRadioTrackInfo(data);
        return data;
    } catch (error) {
        log(`Error fetching radio now playing: ${error.message}`, 'RADIO', true);
        return null;
    }
}

// Update track info with radio focus
function updateRadioTrackInfo(info) {
    try {
        if (info.error) {
            showStatus(`📻 Radio server error: ${info.error}`, true);
            return;
        }
        
        const previousTrackId = radioState.currentTrackId;
        radioState.currentTrack = info;
        
        // Radio position synchronization (server-authoritative)
        if (info.radio_position !== undefined || info.playback_position !== undefined) {
            const serverPosition = info.radio_position || info.playback_position;
            const serverPositionMs = info.radio_position_ms || info.playback_position_ms || 0;
            const now = Date.now();
            
            radioState.radioPosition = serverPosition;
            radioState.radioPositionMs = serverPositionMs;
            radioState.lastRadioUpdate = now;
            radioState.lastTrackInfoTime = now;
            radioState.serverTimestamp = info.server_timestamp || now;
            
            log(`Radio sync: Server position ${serverPosition}s + ${serverPositionMs}ms`, 'RADIO');
        }
        
        // Track change detection
        const newTrackId = info.path;
        if (radioState.currentTrackId !== newTrackId) {
            log(`Radio track changed: ${info.title}`, 'RADIO');
            radioState.currentTrackId = newTrackId;
            radioState.trackChangeDetected = true;
            radioState.trackChangeTime = Date.now();
            
            if (radioState.isPlaying && radioState.audioElement && !radioState.isReconnecting) {
                log("Radio track changed while playing, will reconnect after grace period", 'RADIO');
                
                setTimeout(() => {
                    if (radioState.isPlaying && radioState.trackChangeDetected && !radioState.isReconnecting) {
                        log("Grace period ended, reconnecting for new radio track", 'RADIO');
                        attemptReconnection('track change');
                    }
                }, RADIO_CONFIG.TRACK_CHANGE_GRACE_PERIOD);
            }
        } else {
            radioState.trackChangeDetected = false;
        }
        
        // Update UI
        currentTitle.textContent = info.title || 'Unknown Title';
        currentArtist.textContent = info.artist || 'Unknown Artist';
        currentAlbum.textContent = info.album || 'Unknown Album';
        
        if (info.duration) {
            currentDuration.textContent = formatTime(info.duration);
        }
        
        // Update progress bar with radio position
        if (radioState.currentTrack && radioState.currentTrack.duration) {
            updateProgressBar(radioState.radioPosition, info.duration);
        }
        
        // Update listener count
        if (info.active_listeners !== undefined) {
            listenerCount.innerHTML = `<span class="radio-live">LIVE</span> • Listeners: ${info.active_listeners}`;
        }
        
        // Update document title for radio
        document.title = `📻 ${info.title} - ${info.artist} | ChillOut Radio`;
        
        // Log radio mode confirmation
        if (info.streaming_mode === 'radio') {
            log('Radio mode confirmed by server', 'RADIO');
        }
        
    } catch (e) {
        log(`Error processing radio track info: ${e.message}`, 'RADIO', true);
    }
}

// static/js/radio-player.js - Part 8: iOS Support and Utility Functions

// Unlock iOS audio for radio
function unlockIOSAudio(event) {
    if (radioState.iosPlaybackUnlocked) return;
    
    log("Attempting to unlock iOS audio for radio", 'IOS');
    
    const tempAudio = new Audio();
    tempAudio.src = 'data:audio/mpeg;base64,SUQzBAAAAAAAI1RTU0UAAAAPAAADTGF2ZjU4Ljc2LjEwMAAAAAAAAAAAAAAA//OEAAAAAAAAAAAAAAAAAAAAAAAASW5mbwAAAA8AAAAEAAABIADAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMD/////////////////////wAAABhMYXZjNTguMTM=';
    
    const playPromise = tempAudio.play();
    if (playPromise !== undefined) {
        playPromise.then(() => {
            log("iOS audio unlocked successfully for radio", 'IOS');
            radioState.iosPlaybackUnlocked = true;
            tempAudio.pause();
            tempAudio.src = '';
            
            if (radioState.pendingPlay && radioState.audioElement) {
                radioState.pendingPlay = false;
                startRadioStream();
            }
        }).catch(err => {
            log(`iOS audio unlock failed: ${err.message}`, 'IOS', true);
        });
    }
}

// Update progress bar (radio position only)
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
    if (RADIO_CONFIG.DEBUG_MODE || isError) {
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
        }, radioState.isMobile ? 4000 : 3000);
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

// Enhanced logging for radio
function log(message, category = 'INFO', isError = false) {
    if (isError || RADIO_CONFIG.DEBUG_MODE) {
        const timestamp = new Date().toISOString().substr(11, 8);
        const style = isError 
            ? 'color: #e74c3c; font-weight: bold;' 
            : (category === 'RADIO' ? 'color: #4CAF50; font-weight: bold;' :
               category === 'ANDROID' ? 'color: #FF9800; font-weight: bold;' :
               category === 'IOS' ? 'color: #ff6b6b; font-weight: bold;' :
               category === 'NETWORK' ? 'color: #34495e; font-weight: bold;' :
               category === 'UI' ? 'color: #16a085;' :
               category === 'STORAGE' ? 'color: #95a5a6;' :
               'color: #2c3e50;');
        
        console[isError ? 'error' : 'log'](`%c[${timestamp}] [${category}] ${message}`, style);
    }
}

// static/js/radio-player.js - Part 9: Event Handlers and Final Initialization

// Handle page unload
window.addEventListener('beforeunload', () => {
    log('Radio page unloading, cleaning up', 'RADIO');
    
    // Clear all timers
    const timers = ['nowPlayingTimer', 'connectionHealthTimer', 'heartbeatTimer'];
    timers.forEach(timer => {
        if (radioState[timer]) {
            clearInterval(radioState[timer]);
            radioState[timer] = null;
        }
    });
    
    if (radioState.cleanupTimeout) {
        clearTimeout(radioState.cleanupTimeout);
        radioState.cleanupTimeout = null;
    }
    
    // Save final state
    try {
        if (radioState.volume !== 0.7) {
            localStorage.setItem('radioVolume', radioState.volume.toString());
        }
        if (radioState.isMuted) {
            localStorage.setItem('radioMuted', radioState.isMuted.toString());
        }
    } catch (e) {
        // Ignore storage errors on unload
    }
});

// Handle network changes
window.addEventListener('online', () => {
    log('Network connection restored', 'NETWORK');
    if (radioState.isPlaying && radioState.audioElement && radioState.audioElement.paused) {
        showStatus('📻 Connection restored - reconnecting to radio...', false, true);
        setTimeout(() => {
            attemptReconnection('network restored');
        }, 1000);
    }
});

window.addEventListener('offline', () => {
    log('Network connection lost', 'NETWORK', true);
    showStatus('📻 Network connection lost', true);
});

// Handle page visibility changes for battery optimization
document.addEventListener('visibilitychange', () => {
    if (document.hidden) {
        log('Radio page hidden - reducing activity', 'VISIBILITY');
        if (radioState.isPlaying) {
            // Reduce heartbeat frequency when hidden
            if (radioState.heartbeatTimer) {
                clearInterval(radioState.heartbeatTimer);
                radioState.heartbeatTimer = setInterval(sendHeartbeat, RADIO_CONFIG.MOBILE_HEARTBEAT_INTERVAL * 2);
            }
        }
    } else {
        log('Radio page visible - resuming activity', 'VISIBILITY');
        if (radioState.isPlaying) {
            // Restore normal heartbeat frequency
            if (radioState.heartbeatTimer) {
                clearInterval(radioState.heartbeatTimer);
                radioState.heartbeatTimer = setInterval(sendHeartbeat, RADIO_CONFIG.MOBILE_HEARTBEAT_INTERVAL);
            }
            // Send immediate heartbeat and fetch fresh info
            sendHeartbeat();
            fetchNowPlaying();
        }
    }
});

// Initialize radio player when DOM is ready
document.addEventListener('DOMContentLoaded', () => {
    try {
        initRadioPlayer();
    } catch (error) {
        log(`Failed to initialize radio player: ${error.message}`, 'RADIO', true);
        alert(`Radio player initialization failed: ${error.message}`);
    }
});

// Handle audio interruptions (mobile)
if ('mediaSession' in navigator) {
    try {
        // Set up media session for radio
        navigator.mediaSession.setActionHandler('play', () => {
            if (!radioState.isPlaying) {
                log('Media session play request', 'MEDIA');
                startRadio();
            }
        });
        
        navigator.mediaSession.setActionHandler('pause', () => {
            if (radioState.isPlaying) {
                log('Media session pause request', 'MEDIA');
                stopRadio();
            }
        });
        
        navigator.mediaSession.setActionHandler('stop', () => {
            if (radioState.isPlaying) {
                log('Media session stop request', 'MEDIA');
                stopRadio();
            }
        });
        
        log('Media session handlers registered for radio', 'MEDIA');
    } catch (e) {
        log(`Media session setup failed: ${e.message}`, 'MEDIA');
    }
}

// Update media session metadata when track changes
function updateMediaSession() {
    if (!('mediaSession' in navigator) || !radioState.currentTrack) return;
    
    try {
        navigator.mediaSession.metadata = new MediaMetadata({
            title: radioState.currentTrack.title || 'ChillOut Radio',
            artist: radioState.currentTrack.artist || 'Unknown Artist',
            album: radioState.currentTrack.album || 'Live Stream',
            artwork: [
                { src: '/static/icon-96.png', sizes: '96x96', type: 'image/png' },
                { src: '/static/icon-192.png', sizes: '192x192', type: 'image/png' },
                { src: '/static/icon-512.png', sizes: '512x512', type: 'image/png' }
            ]
        });
        
        if (radioState.currentTrack.duration) {
            navigator.mediaSession.setPositionState({
                duration: radioState.currentTrack.duration,
                playbackRate: 1.0,
                position: radioState.radioPosition
            });
        }
        
        log(`Media session metadata updated: ${radioState.currentTrack.title}`, 'MEDIA');
    } catch (e) {
        log(`Media session metadata update failed: ${e.message}`, 'MEDIA');
    }
}

// Performance monitoring for radio
let performanceCheckInterval;

function startPerformanceMonitoring() {
    if (!RADIO_CONFIG.DEBUG_MODE) return;
    
    performanceCheckInterval = setInterval(() => {
        // Check memory usage if available
        if (performance.memory) {
            const memory = performance.memory;
            const usedMB = Math.round(memory.usedJSHeapSize / 1048576);
            const limitMB = Math.round(memory.jsHeapSizeLimit / 1048576);
            
            if (usedMB > limitMB * 0.8) {
                log(`High memory usage: ${usedMB}MB/${limitMB}MB`, 'PERFORMANCE', true);
                
                // Trigger garbage collection if possible
                if (window.gc) {
                    window.gc();
                    log('Manual garbage collection triggered', 'PERFORMANCE');
                }
            }
        }
        
        // Check for audio element leaks
        const audioElements = document.querySelectorAll('audio');
        if (audioElements.length > 2) {
            log(`Potential audio element leak: ${audioElements.length} elements`, 'PERFORMANCE', true);
        }
        
        // Radio-specific performance checks
        if (radioState.isPlaying) {
            const now = Date.now();
            const timeSinceLastUpdate = now - radioState.lastRadioUpdate;
            
            if (timeSinceLastUpdate > RADIO_CONFIG.NOW_PLAYING_INTERVAL * 2) {
                log(`Radio info stale: ${Math.round(timeSinceLastUpdate/1000)}s since last update`, 'PERFORMANCE', true);
            }
        }
        
    }, 30000); // Check every 30 seconds
}

// Stop performance monitoring
function stopPerformanceMonitoring() {
    if (performanceCheckInterval) {
        clearInterval(performanceCheckInterval);
        performanceCheckInterval = null;
    }
}

// Start performance monitoring if debug mode is enabled
if (RADIO_CONFIG.DEBUG_MODE) {
    startPerformanceMonitoring();
}

// Cleanup on page unload
window.addEventListener('beforeunload', () => {
    stopPerformanceMonitoring();
});

// Log startup message
console.log('%cChillOut Radio - Live Radio Stream v2.2.0', 'color: #4CAF50; font-weight: bold; font-size: 16px;');
console.log('%c📻 Radio-style streaming - all listeners synchronized to current time', 'color: #2196F3; font-style: italic;');
console.log('%c🎵 No seeking, no rewinding - just tune in and enjoy!', 'color: #FF9800; font-style: italic;');

// Global radio object for debugging
if (RADIO_CONFIG.DEBUG_MODE) {
    window.ChillOutRadio = {
        state: radioState,
        config: RADIO_CONFIG,
        controls: {
            start: startRadio,
            stop: stopRadio,
            fetchInfo: fetchNowPlaying,
            reconnect: attemptReconnection
        },
        version: '2.2.0-radio-mode'
    };
    
    console.log('%cDebug mode enabled - window.ChillOutRadio available for debugging', 'color: #9C27B0; font-weight: bold;');
}