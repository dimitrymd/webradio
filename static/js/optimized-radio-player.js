// static/js/optimized-radio-player.js - Fixed iOS buffering and desktop radio positioning

// Radio configuration optimized for all platforms
const RADIO_CONFIG = {
    NOW_PLAYING_INTERVAL: 8000,         // 8 seconds for better responsiveness
    CONNECTION_CHECK_INTERVAL: 6000,    // 6 seconds
    RECONNECT_ATTEMPTS: 5,              
    DEBUG_MODE: true,
    
    // Radio-specific settings
    RADIO_MODE: true,                   
    SEEKING_ENABLED: false,             
    SYNCHRONIZED_PLAYBACK: true,        
    
    // Platform-optimized error handling
    MAX_ERROR_FREQUENCY: 8000,          
    CLEANUP_DELAY: 300,                 // Faster cleanup
    RECONNECT_MIN_DELAY: 1500,          // Faster reconnection
    RECONNECT_MAX_DELAY: 12000,         
    
    // Track transition
    TRACK_CHANGE_GRACE_PERIOD: 2000,    // Shorter grace period
    
    // iOS-specific optimizations
    IOS_BUFFER_TIMEOUT: 6000,           // Shorter timeout for iOS
    IOS_HEARTBEAT_INTERVAL: 10000,      // More frequent heartbeat
    IOS_RECONNECT_DELAY: 1000,          // Faster iOS reconnection
    
    // Desktop optimizations
    DESKTOP_BUFFER_TIMEOUT: 8000,
    DESKTOP_HEARTBEAT_INTERVAL: 12000,
    
    // Mobile general
    MOBILE_BUFFER_TIMEOUT: 8000,       
    MOBILE_HEARTBEAT_INTERVAL: 12000,   
};

// Optimized radio state management
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
    
    // Radio track info (server-authoritative)
    currentTrackId: null,
    currentTrack: null,
    serverRadioPosition: 0,         // Current server radio position
    serverRadioPositionMs: 0,       // Millisecond precision
    serverTimestamp: 0,             // Server timestamp
    trackChangeDetected: false,
    trackChangeTime: 0,
    lastRadioSync: 0,
    
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
    
    // Platform-specific state
    backgroundTime: 0,
    networkType: 'unknown',
    lowPowerMode: false,
    
    // iOS-specific optimizations
    iosPlaybackUnlocked: false,
    pendingPlay: false,
    iosBufferStalls: 0,
    iosLastStallTime: 0,
    
    // Desktop-specific
    desktopOptimized: false,
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

// Initialize the optimized radio player
function initOptimizedRadioPlayer() {
    log("Initializing ChillOut Radio - Optimized Live Stream", 'RADIO');
    log(`Platform: ${radioState.isMobile ? 'Mobile' : 'Desktop'}, iOS: ${radioState.isIOS}, Android: ${radioState.isAndroid}, Safari: ${radioState.isSafari}`, 'RADIO');
    
    // Detect and adapt to network conditions
    detectAndAdaptNetwork();
    
    // Verify UI elements
    if (!startBtn || !muteBtn || !volumeControl || !statusMessage) {
        log("Critical UI elements missing!", 'ERROR', true);
        alert("Radio player initialization failed: UI elements not found");
        return;
    }
    
    // Set up event listeners
    setupOptimizedEventListeners();
    
    // Platform-specific optimizations
    if (radioState.isIOS) {
        setupIOSRadioOptimizations();
    } else if (radioState.isAndroid) {
        setupAndroidRadioOptimizations();
    } else {
        setupDesktopRadioOptimizations();
    }
    
    // Load saved settings
    loadRadioSettings();
    
    // Set up optimized timers
    setupOptimizedTimers();
    
    // Initial track info fetch
    fetchRadioNowPlaying();
    
    // Set up visibility handling
    setupOptimizedVisibilityHandling();
    
    log('Optimized radio player initialized successfully', 'RADIO');
    showRadioStatus('📻 Radio ready - tap to tune in to the live stream', false, false);
}

// Detect and adapt to network conditions
function detectAndAdaptNetwork() {
    if (navigator.connection) {
        const connection = navigator.connection;
        radioState.networkType = connection.effectiveType || 'unknown';
        
        log(`Network: ${radioState.networkType}, downlink: ${connection.downlink || 'unknown'} Mbps`, 'NETWORK');
        
        // Adapt timeouts based on connection
        adaptConfigToNetwork();
        
        // Listen for connection changes
        connection.addEventListener('change', () => {
            const newType = connection.effectiveType;
            if (newType !== radioState.networkType) {
                log(`Network changed: ${radioState.networkType} -> ${newType}`, 'NETWORK');
                radioState.networkType = newType;
                adaptConfigToNetwork();
                
                if (radioState.isPlaying) {
                    showRadioStatus(`📻 Network: ${newType.toUpperCase()}`, false, true);
                }
            }
        });
    }
}

function adaptConfigToNetwork() {
    switch (radioState.networkType) {
        case 'slow-2g':
        case '2g':
            RADIO_CONFIG.NOW_PLAYING_INTERVAL = 15000;
            RADIO_CONFIG.MOBILE_BUFFER_TIMEOUT = 20000;
            RADIO_CONFIG.RECONNECT_MIN_DELAY = 4000;
            break;
        case '3g':
            RADIO_CONFIG.NOW_PLAYING_INTERVAL = 10000;
            RADIO_CONFIG.MOBILE_BUFFER_TIMEOUT = 12000;
            RADIO_CONFIG.RECONNECT_MIN_DELAY = 2000;
            break;
        default: // 4g or better
            RADIO_CONFIG.NOW_PLAYING_INTERVAL = 8000;
            RADIO_CONFIG.MOBILE_BUFFER_TIMEOUT = 8000;
            RADIO_CONFIG.RECONNECT_MIN_DELAY = 1500;
            break;
    }
    
    log(`Adapted to ${radioState.networkType} network`, 'NETWORK');
}

// iOS-specific radio optimizations
function setupIOSRadioOptimizations() {
    log("Setting up iOS radio streaming optimizations", 'IOS');
    
    radioState.iosPlaybackUnlocked = false;
    
    // iOS audio unlock events
    const unlockEvents = ['touchstart', 'touchend', 'click', 'keydown'];
    unlockEvents.forEach(eventType => {
        document.addEventListener(eventType, unlockIOSRadioAudio, { once: true, passive: true });
    });
    
    // iOS-specific wake lock
    if ('wakeLock' in navigator) {
        navigator.wakeLock.request('screen').catch(err => {
            log(`iOS wake lock failed: ${err.message}`, 'IOS');
        });
    }
    
    // iOS memory pressure handling
    if ('onmemorywarning' in window) {
        window.addEventListener('memorywarning', () => {
            log('iOS memory warning - optimizing radio stream', 'IOS');
            if (radioState.isPlaying && !radioState.isReconnecting) {
                // Reduce buffer size for iOS
                attemptRadioReconnection('iOS memory pressure');
            }
        });
    }
}

// Android radio optimizations
function setupAndroidRadioOptimizations() {
    log(`Setting up Android radio optimizations for version ${radioState.androidVersion}`, 'ANDROID');
    
    // Android wake lock
    if ('wakeLock' in navigator) {
        navigator.wakeLock.request('screen').catch(err => {
            log(`Android wake lock failed: ${err.message}`, 'ANDROID');
        });
    }
    
    // Android AudioContext handling
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

// Desktop radio optimizations
function setupDesktopRadioOptimizations() {
    log("Setting up desktop radio optimizations", 'DESKTOP');
    
    radioState.desktopOptimized = true;
    
    // Desktop can handle more frequent updates
    RADIO_CONFIG.NOW_PLAYING_INTERVAL = 6000;
    RADIO_CONFIG.CONNECTION_CHECK_INTERVAL = 5000;
    RADIO_CONFIG.DESKTOP_HEARTBEAT_INTERVAL = 10000;
    
    // Desktop-specific optimizations
    RADIO_CONFIG.DESKTOP_BUFFER_TIMEOUT = 6000;
    RADIO_CONFIG.RECONNECT_MIN_DELAY = 1000;
}

// Set up optimized event listeners
function setupOptimizedEventListeners() {
    startBtn.addEventListener('click', function(e) {
        e.preventDefault();
        radioState.userHasInteracted = true;
        toggleRadioConnection();
    });
    
    muteBtn.addEventListener('click', function(e) {
        e.preventDefault();
        radioState.userHasInteracted = true;
        toggleRadioMute();
    });
    
    volumeControl.addEventListener('input', function(e) {
        radioState.userHasInteracted = true;
        updateRadioVolume(parseFloat(e.target.value));
    });
}

function toggleRadioMute() {
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
}

function updateRadioVolume(volume) {
    radioState.volume = volume;
    
    if (radioState.audioElement && !radioState.isCleaningUp) {
        radioState.audioElement.volume = volume;
    }
    
    try {
        localStorage.setItem('radioVolume', volume.toString());
    } catch (e) {
        // Ignore storage errors
    }
}

// Load radio settings
function loadRadioSettings() {
    try {
        const savedVolume = localStorage.getItem('radioVolume');
        if (savedVolume !== null) {
            const volume = parseFloat(savedVolume);
            volumeControl.value = volume;
            radioState.volume = volume;
        }
        
        const savedMuted = localStorage.getItem('radioMuted');
        if (savedMuted !== null) {
            radioState.isMuted = savedMuted === 'true';
            muteBtn.textContent = radioState.isMuted ? '🔇 Unmute' : '🔊 Mute';
        }
    } catch (e) {
        log(`Error loading radio settings: ${e.message}`, 'STORAGE');
    }
}

// Setup optimized timers
function setupOptimizedTimers() {
    // Clear existing timers
    clearRadioTimers();
    
    // Platform-specific timer intervals
    const nowPlayingInterval = radioState.isIOS ? RADIO_CONFIG.IOS_HEARTBEAT_INTERVAL : 
                              radioState.isMobile ? RADIO_CONFIG.MOBILE_HEARTBEAT_INTERVAL :
                              RADIO_CONFIG.NOW_PLAYING_INTERVAL;
    
    const heartbeatInterval = radioState.isIOS ? RADIO_CONFIG.IOS_HEARTBEAT_INTERVAL :
                             radioState.isMobile ? RADIO_CONFIG.MOBILE_HEARTBEAT_INTERVAL :
                             RADIO_CONFIG.DESKTOP_HEARTBEAT_INTERVAL;
    
    // Set up timers
    radioState.nowPlayingTimer = setInterval(fetchRadioNowPlaying, nowPlayingInterval);
    radioState.heartbeatTimer = setInterval(sendRadioHeartbeat, heartbeatInterval);
    
    log(`Radio timers configured: nowPlaying=${nowPlayingInterval}ms, heartbeat=${heartbeatInterval}ms`, 'RADIO');
}

function clearRadioTimers() {
    const timers = ['nowPlayingTimer', 'connectionHealthTimer', 'heartbeatTimer'];
    timers.forEach(timer => {
        if (radioState[timer]) {
            clearInterval(radioState[timer]);
            radioState[timer] = null;
        }
    });
}

// Optimized visibility handling
function setupOptimizedVisibilityHandling() {
    document.addEventListener('visibilitychange', function() {
        if (document.hidden) {
            radioState.backgroundTime = Date.now();
            log('Radio app went to background', 'VISIBILITY');
            
            if (radioState.isPlaying) {
                // Reduce activity when in background
                adaptTimersForBackground();
            }
        } else {
            if (radioState.backgroundTime > 0) {
                const backgroundDuration = Date.now() - radioState.backgroundTime;
                log(`Radio app returned to foreground after ${Math.round(backgroundDuration/1000)}s`, 'VISIBILITY');
                
                if (radioState.isPlaying) {
                    // Restore normal activity
                    setupOptimizedTimers();
                    
                    // Check if audio is still playing
                    setTimeout(() => {
                        if (radioState.audioElement && radioState.audioElement.paused && radioState.isPlaying) {
                            log('Radio audio paused during background, attempting recovery', 'VISIBILITY');
                            attemptRadioReconnection('background recovery');
                        } else {
                            fetchRadioNowPlaying();
                        }
                    }, 1000);
                }
                radioState.backgroundTime = 0;
            }
        }
    });
}

function adaptTimersForBackground() {
    // Extend intervals when in background
    clearRadioTimers();
    radioState.nowPlayingTimer = setInterval(fetchRadioNowPlaying, 30000); // 30 seconds
    radioState.heartbeatTimer = setInterval(sendRadioHeartbeat, 45000); // 45 seconds
    log('Radio timers adapted for background mode', 'VISIBILITY');
}

// Toggle radio connection
function toggleRadioConnection() {
    const isConnected = startBtn.dataset.connected === 'true';
    
    if (isConnected) {
        log('User requested radio disconnect', 'RADIO');
        stopRadioStream();
    } else {
        log('User requested radio connect', 'RADIO');
        startRadioStream();
    }
}

// Start radio streaming - ALWAYS from current server position
function startRadioStream() {
    log('Starting radio stream (always from current server time)', 'RADIO');
    
    if (radioState.isPlaying || radioState.isReconnecting) {
        log('Already playing or reconnecting, ignoring start request', 'RADIO');
        return;
    }
    
    startBtn.disabled = true;
    showRadioStatus('📻 Tuning in to radio stream...', false, false);
    
    // Reset state
    radioState.isPlaying = true;
    radioState.isReconnecting = false;
    radioState.reconnectAttempts = 0;
    radioState.trackChangeDetected = false;
    radioState.pendingPlay = false;
    radioState.consecutiveErrors = 0;
    radioState.iosBufferStalls = 0;
    
    // ALWAYS get current server radio position first
    fetchRadioNowPlaying().then(() => {
        log(`Radio tuning: Server position ${radioState.serverRadioPosition}s + ${radioState.serverRadioPositionMs}ms`, 'RADIO');
        
        // Clean up and create new audio element
        cleanupRadioAudioElement().then(() => {
            createOptimizedRadioAudioElement();
            startRadioStreamPlayback();
            setupPlayingTimers();
        });
    }).catch(() => {
        // If fetch fails, still try to connect (server will provide current position)
        log('Failed to fetch current radio position, connecting with server-determined position', 'RADIO');
        
        cleanupRadioAudioElement().then(() => {
            createOptimizedRadioAudioElement();
            startRadioStreamPlayback();
            setupPlayingTimers();
        });
    });
}

// Create optimized radio audio element
function createOptimizedRadioAudioElement() {
    if (radioState.audioElement && !radioState.isCleaningUp) {
        log('Radio audio element already exists', 'RADIO');
        return;
    }
    
    log(`Creating optimized radio audio element for ${radioState.isIOS ? 'iOS' : radioState.isAndroid ? 'Android' : 'desktop'}`, 'RADIO');
    
    radioState.audioElement = new Audio();
    radioState.audioElement.controls = false;
    radioState.audioElement.volume = radioState.volume;
    radioState.audioElement.muted = radioState.isMuted;
    radioState.audioElement.crossOrigin = "anonymous";
    
    // Platform-specific radio settings
    if (radioState.isIOS) {
        // iOS radio optimizations - prevent aggressive buffering
        radioState.audioElement.preload = 'metadata'; // Minimal preload for iOS radio
        radioState.audioElement.autoplay = false;
        
        if (radioState.audioElement.setAttribute) {
            radioState.audioElement.setAttribute('webkit-playsinline', 'true');
            radioState.audioElement.setAttribute('playsinline', 'true');
            // iOS-specific radio attributes
            radioState.audioElement.setAttribute('x-webkit-airplay', 'allow');
        }
        
        radioState.audioElement.playsInline = true;
    } else if (radioState.isAndroid) {
        // Android radio optimizations
        radioState.audioElement.preload = 'auto';
        radioState.audioElement.autoplay = false;
        
        if (radioState.audioElement.setAttribute) {
            radioState.audioElement.setAttribute('webkit-playsinline', 'true');
            radioState.audioElement.setAttribute('playsinline', 'true');
        }
    } else {
        // Desktop radio settings
        radioState.audioElement.preload = 'auto';
        radioState.audioElement.autoplay = false;
    }
    
    // Set up optimized audio event listeners
    setupOptimizedRadioAudioListeners();
    
    log(`Optimized radio audio element created`, 'RADIO');
}

// Setup optimized radio audio event listeners
function setupOptimizedRadioAudioListeners() {
    if (!radioState.audioElement) return;
    
    radioState.audioElement.addEventListener('playing', () => {
        log('Radio playing', 'RADIO');
        showRadioStatus('📻 Tuned in to ChillOut Radio');
        radioState.trackChangeDetected = false;
        radioState.pendingPlay = false;
        radioState.consecutiveErrors = 0;
        radioState.iosBufferStalls = 0; // Reset iOS stall counter
        
        // Send heartbeat to confirm connection
        if (radioState.connectionId) {
            sendRadioHeartbeat();
        }
    });
    
    radioState.audioElement.addEventListener('waiting', () => {
        log('Radio buffering', 'RADIO');
        showRadioStatus('📻 Buffering radio stream...', false, false);
    });
    
    radioState.audioElement.addEventListener('stalled', () => {
        const now = Date.now();
        log('Radio stalled', 'RADIO');
        
        if (radioState.isIOS) {
            // iOS-specific stall handling
            radioState.iosBufferStalls++;
            radioState.iosLastStallTime = now;
            
            log(`iOS radio stall #${radioState.iosBufferStalls}`, 'IOS');
            
            if (radioState.iosBufferStalls >= 3 && (now - radioState.iosLastStallTime) < 10000) {
                // Too many stalls in short time - reconnect with smaller buffer
                log('iOS: Too many radio stalls, reconnecting with optimized settings', 'IOS');
                attemptRadioReconnection('iOS excessive stalling');
                return;
            }
        }
        
        showRadioStatus('📻 Radio signal weak - buffering', true, false);
        
        if (!radioState.isReconnecting && !radioState.trackChangeDetected) {
            const bufferTimeout = radioState.isIOS ? RADIO_CONFIG.IOS_BUFFER_TIMEOUT :
                                 radioState.isMobile ? RADIO_CONFIG.MOBILE_BUFFER_TIMEOUT :
                                 RADIO_CONFIG.DESKTOP_BUFFER_TIMEOUT;
            
            setTimeout(() => {
                if (radioState.isPlaying && !radioState.isReconnecting && 
                    radioState.audioElement && radioState.audioElement.readyState < 3) {
                    log('Radio still stalled after timeout, attempting reconnection', 'RADIO');
                    attemptRadioReconnection('radio stalled');
                }
            }, bufferTimeout);
        }
    });
    
    radioState.audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        const errorMsg = getRadioErrorMessage(e.target.error);
        
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
            
            showRadioStatus('📻 Track ended - tuning to next song', false, false);
            attemptRadioReconnection('track ended');
        }
    });
    
    // Radio progress monitoring (display only - no seeking)
    radioState.audioElement.addEventListener('timeupdate', () => {
        if (radioState.audioElement && !radioState.isCleaningUp && radioState.currentTrack) {
            // Display current radio position from server
            updateRadioProgressBar(radioState.serverRadioPosition, radioState.currentTrack.duration);
        }
    });
    
    // iOS-specific event listeners
    if (radioState.isIOS) {
        radioState.audioElement.addEventListener('canplay', () => {
            log('iOS: Radio can play (buffer sufficient)', 'IOS');
            radioState.iosBufferStalls = Math.max(0, radioState.iosBufferStalls - 1); // Reduce stall count on success
        });
        
        radioState.audioElement.addEventListener('canplaythrough', () => {
            log('iOS: Radio can play through (full buffer)', 'IOS');
            radioState.iosBufferStalls = 0; // Reset stall count on full buffer
        });
        
        radioState.audioElement.addEventListener('suspend', () => {
            log('iOS: Radio loading suspended by browser', 'IOS');
            // iOS Safari sometimes suspends loading - try to resume for radio
            setTimeout(() => {
                if (radioState.audioElement && radioState.isPlaying && 
                    radioState.audioElement.networkState === HTMLMediaElement.NETWORK_LOADING) {
                    radioState.audioElement.load(); // Force resume loading
                }
            }, 1000);
        });
    }
}

// Start radio stream playback - ALWAYS from current server position
function startRadioStreamPlayback() {
    if (!radioState.audioElement) {
        log('No audio element for radio streaming', 'RADIO', true);
        return;
    }
    
    try {
        const timestamp = Date.now();
        
        log(`Starting radio stream from current server time`, 'RADIO');
        
        // Create radio stream URL - NO position parameter (server determines current time)
        let streamUrl = `/direct-stream?t=${timestamp}`;
        
        // Add platform identification for optimization
        if (radioState.isIOS) {
            streamUrl += '&platform=ios&ios_optimized=true';
            // iOS radio optimizations - smaller buffers to prevent stalling
            streamUrl += '&chunk_size=16384'; // 16KB chunks for iOS radio
            streamUrl += '&initial_buffer=32768'; // 32KB initial buffer
            streamUrl += '&min_buffer_time=1'; // 1 second minimum buffer
        } else if (radioState.isAndroid) {
            streamUrl += '&platform=android';
        } else if (radioState.isMobile) {
            streamUrl += '&platform=mobile';
        } else {
            streamUrl += '&platform=desktop';
        }
        
        log(`Radio stream URL: ${streamUrl}`, 'RADIO');
        
        // Set source
        radioState.audioElement.src = streamUrl;
        
        log('Starting radio playback attempt', 'RADIO');
        showRadioStatus('📻 Connecting to radio stream...', false, false);
        
        // Platform-specific playback timing
        const playDelay = radioState.isIOS ? 500 : // Shorter delay for iOS to reduce stalling
                         radioState.isMobile ? 600 : 
                         200; // Desktop can start quickly
        
        setTimeout(() => {
            if (radioState.audioElement && radioState.isPlaying && !radioState.isCleaningUp) {
                attemptRadioPlayback();
            }
        }, playDelay);
        
    } catch (e) {
        log(`Radio streaming setup error: ${e.message}`, 'RADIO', true);
        showRadioStatus(`📻 Radio streaming error: ${e.message}`, true);
        stopRadioStream(true);
    }
}

// Attempt radio playback with platform-specific handling
function attemptRadioPlayback() {
    const playPromise = radioState.audioElement.play();
    if (playPromise !== undefined) {
        playPromise.then(() => {
            log(`Radio playback started successfully`, 'RADIO');
            showRadioStatus('📻 Tuned in to ChillOut Radio');
            startBtn.textContent = '📻 Disconnect';
            startBtn.disabled = false;
            startBtn.dataset.connected = 'true';
            
        }).catch(e => {
            log(`Radio playback failed: ${e.message}`, 'RADIO', true);
            handleRadioPlaybackFailure(e);
        });
    }
}

// Handle radio playback failures
function handleRadioPlaybackFailure(error) {
    log(`Radio playback failure: ${error.name} - ${error.message}`, 'RADIO', true);
    
    if (error.name === 'NotAllowedError') {
        showRadioStatus('📻 Please tap to enable radio audio playback', true, false);
        startBtn.disabled = false;
        startBtn.textContent = '🔊 Enable Audio';
        startBtn.onclick = function() {
            radioState.userHasInteracted = true;
            
            // Unlock iOS audio if needed
            if (radioState.isIOS && !radioState.iosPlaybackUnlocked) {
                unlockIOSRadioAudio().then(() => {
                    attemptRadioPlayback();
                });
            } else {
                attemptRadioPlayback();
            }
        };
    } else {
        showRadioStatus(`📻 Radio playback failed - ${error.message}`, true);
        startBtn.disabled = false;
        
        const retryDelay = radioState.isIOS ? RADIO_CONFIG.IOS_RECONNECT_DELAY :
                          radioState.isMobile ? 3000 : 2000;
        
        setTimeout(() => {
            if (radioState.isPlaying && !radioState.isReconnecting) {
                attemptRadioReconnection('radio playback failure');
            }
        }, retryDelay);
    }
}

// Unlock iOS audio for radio
function unlockIOSRadioAudio() {
    return new Promise((resolve, reject) => {
        if (radioState.iosPlaybackUnlocked) {
            resolve();
            return;
        }
        
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
                resolve();
            }).catch(err => {
                log(`iOS audio unlock failed: ${err.message}`, 'IOS', true);
                reject(err);
            });
        } else {
            resolve();
        }
    });
}

// Setup timers for active radio playback
function setupPlayingTimers() {
    clearRadioTimers();
    
    // Platform-optimized intervals for active playback
    const nowPlayingInterval = radioState.isIOS ? 6000 : // More frequent for iOS
                              radioState.isMobile ? 8000 :
                              6000; // Frequent for desktop
    
    const healthCheckInterval = radioState.isIOS ? 4000 : // Frequent health checks for iOS
                               radioState.isMobile ? 6000 :
                               5000;
    
    radioState.nowPlayingTimer = setInterval(fetchRadioNowPlaying, nowPlayingInterval);
    radioState.connectionHealthTimer = setInterval(checkRadioConnectionHealth, healthCheckInterval);
    
    log(`Radio playing timers: nowPlaying=${nowPlayingInterval}ms, health=${healthCheckInterval}ms`, 'RADIO');
}

// Fetch now playing with radio focus
async function fetchRadioNowPlaying() {
    try {
        log("Fetching radio now playing information", 'API');
        
        let apiUrl = '/api/now-playing';
        if (radioState.isIOS) {
            apiUrl += '?mobile_client=true&ios_client=true';
        } else if (radioState.isMobile) {
            apiUrl += '?mobile_client=true';
        }
        
        const response = await fetch(apiUrl, {
            headers: {
                'Cache-Control': 'no-cache, no-store, must-revalidate',
                'Pragma': 'no-cache'
            }
        });
        
        if (!response.ok) {
            log(`Radio now playing API error: ${response.status}`, 'API', true);
            return null;
        }
        
        const data = await response.json();
        updateRadioTrackInfo(data);
        return data;
    } catch (error) {
        log(`Error fetching radio now playing: ${error.message}`, 'API', true);
        return null;
    }
}

// Update track info with radio synchronization
function updateRadioTrackInfo(info) {
    try {
        if (info.error) {
            showRadioStatus(`📻 Radio server error: ${info.error}`, true);
            return;
        }
        
        const previousTrackId = radioState.currentTrackId;
        radioState.currentTrack = info;
        
        // Radio position synchronization (server-authoritative)
        if (info.radio_position !== undefined || info.playback_position !== undefined) {
            const serverPosition = info.radio_position || info.playback_position;
            const serverPositionMs = info.radio_position_ms || info.playback_position_ms || 0;
            const now = Date.now();
            
            radioState.serverRadioPosition = serverPosition;
            radioState.serverRadioPositionMs = serverPositionMs;
            radioState.lastRadioSync = now;
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
                        attemptRadioReconnection('track change');
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
            updateRadioProgressBar(radioState.serverRadioPosition, info.duration);
        }
        
        // Update listener count
        if (info.active_listeners !== undefined) {
            listenerCount.innerHTML = `<span class="radio-live">LIVE</span> • Listeners: ${info.active_listeners}`;
        }
        
        // Update document title for radio
        document.title = `📻 ${info.title} - ${info.artist} | ChillOut Radio`;
        
    } catch (e) {
        log(`Error processing radio track info: ${e.message}`, 'RADIO', true);
    }
}

// Send radio heartbeat
async function sendRadioHeartbeat() {
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
                radioState.serverRadioPosition = data.radio_position;
                radioState.serverRadioPositionMs = data.radio_position_ms || 0;
                radioState.lastRadioSync = Date.now();
            }
        }
    } catch (error) {
        log(`Radio heartbeat failed: ${error.message}`, 'HEARTBEAT');
    }
}

// Radio connection health check
function checkRadioConnectionHealth() {
    if (!radioState.isPlaying || radioState.isReconnecting) return;
    
    const now = Date.now();
    const timeSinceLastSync = (now - radioState.lastRadioSync) / 1000;
    const timeSinceLastHeartbeat = (now - radioState.lastHeartbeat) / 1000;
    
    // Check if we need fresh track info
    if (timeSinceLastSync > 15) { // 15 seconds without sync
        fetchRadioNowPlaying();
    }
    
    // Check if heartbeat is too old
    if (timeSinceLastHeartbeat > 30) { // 30 seconds without heartbeat
        sendRadioHeartbeat();
    }
    
    if (radioState.audioElement && !radioState.isCleaningUp) {
        // Platform-specific health checks
        if (radioState.audioElement.paused && radioState.isPlaying && !radioState.trackChangeDetected) {
            log('Radio: Audio is paused unexpectedly', 'RADIO', true);
            
            const playPromise = radioState.audioElement.play();
            if (playPromise !== undefined) {
                playPromise.then(() => {
                    log('Radio: Successfully resumed paused audio', 'RADIO');
                }).catch(e => {
                    log(`Radio: Resume failed, will reconnect: ${e.message}`, 'RADIO');
                    attemptRadioReconnection('radio unexpected pause');
                });
            }
        }
        
        if (radioState.audioElement.networkState === HTMLMediaElement.NETWORK_NO_SOURCE) {
            log('Radio: Audio has no source', 'RADIO', true);
            attemptRadioReconnection('radio no source');
        }
        
        // iOS-specific health checks
        if (radioState.isIOS && radioState.audioElement.readyState < 2 && !radioState.trackChangeDetected) {
            const timeSinceLastStall = now - radioState.iosLastStallTime;
            if (timeSinceLastStall > 5000) { // 5 seconds since last stall
                log('iOS: Radio readyState indicates loading issues', 'IOS');
                // Don't immediately reconnect, but prepare for it
            }
        }
    }
}

// Handle radio errors
function handleRadioError(errorCode, errorMsg) {
    log(`Radio error handler: code ${errorCode}, message: ${errorMsg}`, 'RADIO', true);
    
    let reconnectDelay = RADIO_CONFIG.RECONNECT_MIN_DELAY;
    
    if (radioState.consecutiveErrors > 3) {
        reconnectDelay = Math.min(RADIO_CONFIG.RECONNECT_MAX_DELAY, reconnectDelay * radioState.consecutiveErrors);
        showRadioStatus(`📻 Radio signal issues - waiting ${Math.round(reconnectDelay/1000)}s before retry`, true, false);
    } else if (errorCode === 4) { // MEDIA_ERR_SRC_NOT_SUPPORTED
        showRadioStatus('📻 Radio format issue - getting fresh signal...', true, false);
        reconnectDelay = radioState.isIOS ? RADIO_CONFIG.IOS_RECONNECT_DELAY :
                        radioState.isMobile ? 2000 : 1500;
    } else if (errorCode === 2) { // MEDIA_ERR_NETWORK
        showRadioStatus('📻 Network error - reconnecting to radio...', true, false);
        reconnectDelay = radioState.networkType === '2g' ? 5000 : 
                        radioState.isIOS ? RADIO_CONFIG.IOS_RECONNECT_DELAY :
                        radioState.isMobile ? 2000 : 1500;
    } else {
        showRadioStatus('📻 Radio error - will reconnect', true, false);
        reconnectDelay = radioState.isIOS ? RADIO_CONFIG.IOS_RECONNECT_DELAY :
                        radioState.isMobile ? 2000 : 1500;
    }
    
    setTimeout(() => {
        if (radioState.isPlaying && !radioState.isReconnecting) {
            attemptRadioReconnection(`radio error code ${errorCode}`);
        }
    }, reconnectDelay);
}

// Attempt radio reconnection with platform optimizations
function attemptRadioReconnection(reason = 'unknown') {
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
        showRadioStatus('📻 Could not reconnect to radio. Please try again later.', true);
        stopRadioStream(true);
        return;
    }
    
    radioState.isReconnecting = true;
    radioState.reconnectAttempts++;
    
    // Platform-optimized reconnection delay
    const baseDelay = radioState.isIOS ? RADIO_CONFIG.IOS_RECONNECT_DELAY :
                     RADIO_CONFIG.RECONNECT_MIN_DELAY;
    
    const backoffDelay = Math.min(
        baseDelay * Math.pow(1.2, radioState.reconnectAttempts - 1), 
        RADIO_CONFIG.RECONNECT_MAX_DELAY
    );
    
    let networkMultiplier = 1;
    if (radioState.networkType === '2g' || radioState.networkType === 'slow-2g') {
        networkMultiplier = 2;
    } else if (radioState.networkType === '3g') {
        networkMultiplier = 1.3;
    }
    
    const delay = (backoffDelay * networkMultiplier) + (Math.random() * 500);
    
    log(`Radio reconnection attempt ${radioState.reconnectAttempts}/${RADIO_CONFIG.RECONNECT_ATTEMPTS} in ${Math.round(delay/1000)}s (reason: ${reason})`, 'RADIO');
    showRadioStatus(`📻 Reconnecting to radio (${radioState.reconnectAttempts}/${RADIO_CONFIG.RECONNECT_ATTEMPTS})...`, true, false);
    
    cleanupRadioAudioElement().then(() => {
        setTimeout(() => {
            if (!radioState.isPlaying) {
                radioState.isReconnecting = false;
                return;
            }
            
            log(`Executing radio reconnection attempt ${radioState.reconnectAttempts}`, 'RADIO');
            
            createOptimizedRadioAudioElement();
            
            // Always fetch fresh position before reconnecting
            fetchRadioNowPlaying().then(() => {
                if (radioState.isPlaying && radioState.audioElement) {
                    startRadioStreamPlayback();
                }
                
                setTimeout(() => {
                    radioState.isReconnecting = false;
                }, 2000);
            }).catch(() => {
                if (radioState.isPlaying && radioState.audioElement) {
                    startRadioStreamPlayback();
                }
                radioState.isReconnecting = false;
            });
        }, delay);
    });
}

// Stop radio stream
function stopRadioStream(isError = false) {
    log(`Stopping radio stream${isError ? ' (due to error)' : ''}`, 'RADIO');
    
    radioState.isPlaying = false;
    radioState.isReconnecting = false;
    radioState.pendingPlay = false;
    
    // Clear all timers
    clearRadioTimers();
    
    cleanupRadioAudioElement().then(() => {
        log('Radio audio cleanup completed', 'RADIO');
    });
    
    if (!isError) {
        showRadioStatus('📻 Disconnected from radio stream');
    }
    
    // Reset UI
    startBtn.textContent = '📻 Tune In';
    startBtn.disabled = false;
    startBtn.dataset.connected = 'false';
    startBtn.onclick = toggleRadioConnection;
}

// Enhanced cleanup for radio
function cleanupRadioAudioElement() {
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
        
        // Platform-optimized cleanup delay
        const cleanupDelay = radioState.isIOS ? RADIO_CONFIG.CLEANUP_DELAY * 2 :
                            radioState.isMobile ? RADIO_CONFIG.CLEANUP_DELAY * 1.5 :
                            RADIO_CONFIG.CLEANUP_DELAY;
        
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

// Update radio progress bar (display only)
function updateRadioProgressBar(position, duration) {
    if (progressBar && duration > 0) {
        const percent = (position / duration) * 100;
        progressBar.style.width = `${percent}%`;
        
        if (currentPosition) currentPosition.textContent = formatTime(position);
        if (currentDuration) currentDuration.textContent = formatTime(duration);
    }
}

// Show radio status message
function showRadioStatus(message, isError = false, autoHide = true) {
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

// Get radio error message
function getRadioErrorMessage(error) {
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

// Format time
function formatTime(seconds) {
    if (!seconds || seconds < 0) return '0:00';
    const minutes = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

// Enhanced logging for radio
function log(message, category = 'INFO', isError = false) {
    if (isError || RADIO_CONFIG.DEBUG_MODE) {
        const timestamp = new Date().toISOString().substr(11, 8);
        const style = isError 
            ? 'color: #e74c3c; font-weight: bold;' 
            : (category === 'RADIO' ? 'color: #4CAF50; font-weight: bold;' :
               category === 'IOS' ? 'color: #ff6b6b; font-weight: bold;' :
               category === 'ANDROID' ? 'color: #FF9800; font-weight: bold;' :
               category === 'DESKTOP' ? 'color: #9C27B0; font-weight: bold;' :
               category === 'NETWORK' ? 'color: #34495e; font-weight: bold;' :
               category === 'API' ? 'color: #3498db;' :
               category === 'HEARTBEAT' ? 'color: #e67e22;' :
               category === 'VISIBILITY' ? 'color: #1abc9c;' :
               category === 'UI' ? 'color: #16a085;' :
               category === 'STORAGE' ? 'color: #95a5a6;' :
               'color: #2c3e50;');
        
        console[isError ? 'error' : 'log'](`%c[${timestamp}] [${category}] ${message}`, style);
    }
}

// Event handlers
window.addEventListener('beforeunload', () => {
    log('Radio page unloading, cleaning up', 'RADIO');
    clearRadioTimers();
    
    if (radioState.cleanupTimeout) {
        clearTimeout(radioState.cleanupTimeout);
        radioState.cleanupTimeout = null;
    }
    
    // Save final state
    try {
        localStorage.setItem('radioVolume', radioState.volume.toString());
        localStorage.setItem('radioMuted', radioState.isMuted.toString());
    } catch (e) {
        // Ignore storage errors on unload
    }
});

// Network event handlers
window.addEventListener('online', () => {
    log('Network connection restored', 'NETWORK');
    if (radioState.isPlaying && radioState.audioElement && radioState.audioElement.paused) {
        showRadioStatus('📻 Connection restored - reconnecting to radio...', false, true);
        setTimeout(() => {
            attemptRadioReconnection('network restored');
        }, 1000);
    }
});

window.addEventListener('offline', () => {
    log('Network connection lost', 'NETWORK', true);
    showRadioStatus('📻 Network connection lost', true);
});

// Media session API for radio
if ('mediaSession' in navigator) {
    try {
        navigator.mediaSession.setActionHandler('play', () => {
            if (!radioState.isPlaying) {
                log('Media session play request', 'MEDIA');
                startRadioStream();
            }
        });
        
        navigator.mediaSession.setActionHandler('pause', () => {
            if (radioState.isPlaying) {
                log('Media session pause request', 'MEDIA');
                stopRadioStream();
            }
        });
        
        navigator.mediaSession.setActionHandler('stop', () => {
            if (radioState.isPlaying) {
                log('Media session stop request', 'MEDIA');
                stopRadioStream();
            }
        });
        
        log('Media session handlers registered for radio', 'MEDIA');
    } catch (e) {
        log(`Media session setup failed: ${e.message}`, 'MEDIA');
    }
}

// Update media session metadata
function updateRadioMediaSession() {
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
                position: radioState.serverRadioPosition
            });
        }
        
        log(`Media session metadata updated: ${radioState.currentTrack.title}`, 'MEDIA');
    } catch (e) {
        log(`Media session metadata update failed: ${e.message}`, 'MEDIA');
    }
}

// Initialize radio player when DOM is ready
document.addEventListener('DOMContentLoaded', () => {
    try {
        initOptimizedRadioPlayer();
    } catch (error) {
        log(`Failed to initialize optimized radio player: ${error.message}`, 'RADIO', true);
        alert(`Radio player initialization failed: ${error.message}`);
    }
});

// Global radio object for debugging
if (RADIO_CONFIG.DEBUG_MODE) {
    window.ChillOutRadio = {
        state: radioState,
        config: RADIO_CONFIG,
        controls: {
            start: startRadioStream,
            stop: stopRadioStream,
            fetchInfo: fetchRadioNowPlaying,
            reconnect: attemptRadioReconnection,
            heartbeat: sendRadioHeartbeat
        },
        version: '2.3.0-optimized-radio',
        platform: {
            isIOS: radioState.isIOS,
            isAndroid: radioState.isAndroid,
            isMobile: radioState.isMobile,
            optimized: true
        }
    };
    
    console.log('%cChillOut Radio - Optimized Live Stream v2.3.0', 'color: #4CAF50; font-weight: bold; font-size: 16px;');
    console.log('%c📻 Fixed: iOS buffering issues, Desktop radio positioning, Android sync', 'color: #2196F3; font-style: italic;');
    console.log('%c🎵 Radio mode: All listeners synchronized to current server time', 'color: #FF9800; font-style: italic;');
    console.log('%cDebug mode enabled - window.ChillOutRadio available', 'color: #9C27B0; font-weight: bold;');
}