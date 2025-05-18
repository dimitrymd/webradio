// player-core.js - Enhanced platform detection with capabilities assessment

// Global state object to track player state
window.state = {
    // Audio element
    audioElement: null,
    mediaSource: null,
    sourceBuffer: null,
    
    // Connection state
    isPlaying: false,
    isLoading: false,
    usingDirectStream: true, // Default to direct streaming for all platforms
    
    // Playback state
    currentTrackId: null,
    trackDuration: 0,
    streamStartTime: 0,
    startPosition: 0,
    
    // Buffer management
    audioQueue: [],
    bufferUnderflows: 0,
    lastBufferEvent: 0,
    lastErrorTime: 0,
    bufferMetrics: [],
    
    // UI state
    isMuted: false,
    lastToggle: 0,
    statusHideTimer: null,
    debugMode: true, // Enable for development
    
    // Platform detection (will be populated during initialization)
    isIOS: false,
    isAndroid: false,
    isMobile: false,
    
    // Track timing and monitoring
    lastTrackInfoTime: 0,
    lastAudioChunkTime: 0,
    trackPlaybackDuration: 0,
    
    // Reconnection handling
    reconnectAttempts: 0,
    maxReconnectAttempts: 10,
    
    // Error tracking
    errorHistory: [],
    consecutiveErrors: 0,
    lastErrorMessage: '',
    
    // Performance metrics
    performanceMetrics: {
        avgBufferSize: 0,
        bufferSamples: 0,
        lastBufferCheck: 0
    }
};

// Global config object for player settings
window.config = {
    // Buffer size settings (in seconds)
    MIN_BUFFER_SIZE: 2,        // Minimum buffer before playback starts
    TARGET_BUFFER_SIZE: 10,    // Target buffer size to maintain
    MAX_BUFFER_SIZE: 30,       // Maximum buffer size before trimming
    
    // Mobile-specific buffer multipliers
    MOBILE_BUFFER_MULTIPLIER: 1.5,  // Mobile devices need more buffer
    IOS_BUFFER_MULTIPLIER: 1.5,     // iOS needs even more buffer
    
    // Connection management
    RECONNECT_DELAY_BASE: 1000,     // 1 second base delay
    RECONNECT_BACKOFF_FACTOR: 1.5,  // Exponential backoff factor
    NO_DATA_TIMEOUT: 15,            // Seconds without data before reconnecting
    
    // Track position synchronization
    POSITION_SYNC_THRESHOLD: 5,     // Seconds difference between client/server before resyncing
    MIN_TRACK_PLAYBACK_TIME: 10,    // Minimum seconds to play before considering track change
    
    // Timing and polling
    NOW_PLAYING_INTERVAL: 10000,    // ms between polling for now playing info
    
    // Buffer management
    AUDIO_STARVATION_THRESHOLD: 0.5 // Seconds of buffer before considering "starved"
};

// UI element references (will be populated during init)
let startBtn = null;
let muteBtn = null;
let volumeControl = null;
let progressBar = null;
let currentPosition = null;
let currentDuration = null;
let currentTitle = null;
let currentArtist = null;
let currentAlbum = null;
let listenerCount = null;
let statusMessage = null;

// Enhanced platform detection with capabilities assessment
function detectPlatform() {
    const ua = window.navigator.userAgent;
    
    // iOS detection with version information
    let iOSVersion = 0;
    let iOS = /iPad|iPhone|iPod/.test(ua) || 
              (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
    
    if (iOS) {
        // Extract iOS version if possible
        const match = ua.match(/OS (\d+)_(\d+)/);
        if (match && match[1]) {
            iOSVersion = parseInt(match[1], 10);
        }
        
        log(`Detected iOS ${iOSVersion || 'unknown'} device: ${ua}`, 'PLATFORM');
    }
    
    // Android detection with version
    let isAndroid = /Android/.test(ua);
    let androidVersion = 0;
    
    if (isAndroid) {
        const match = ua.match(/Android (\d+)\.(\d+)/);
        if (match && match[1]) {
            androidVersion = parseInt(match[1], 10);
        }
        log(`Detected Android ${androidVersion || 'unknown'} device: ${ua}`, 'PLATFORM');
    }
    
    // General mobile detection
    const mobile = iOS || isAndroid || /webOS|BlackBerry|IEMobile|Opera Mini/i.test(ua);
    
    // Browser detection
    const isChrome = /Chrome/.test(ua) && /Google Inc/.test(navigator.vendor);
    const isSafari = /Safari/.test(ua) && /Apple Computer/.test(navigator.vendor);
    const isFirefox = /Firefox/.test(ua);
    const isEdge = /Edg/.test(ua);
    
    // Connection type detection if available
    let connectionType = 'unknown';
    let isSlowConnection = false;
    
    if ('connection' in navigator) {
        const connection = navigator.connection;
        
        if (connection) {
            connectionType = connection.effectiveType || 'unknown';
            
            // Consider slow connections
            isSlowConnection = connectionType === 'slow-2g' || 
                               connectionType === '2g' || 
                               (connection.downlink && connection.downlink < 1.5);
                               
            log(`Network: ${connectionType}, downlink: ${connection.downlink || 'unknown'} Mbps`, 'PLATFORM');
        }
    }
    
    // Store in state
    state.isIOS = iOS;
    state.iOSVersion = iOSVersion;
    state.isAndroid = isAndroid;
    state.androidVersion = androidVersion;
    state.isMobile = mobile;
    state.isChrome = isChrome;
    state.isSafari = isSafari;
    state.isFirefox = isFirefox;
    state.isEdge = isEdge;
    state.connectionType = connectionType;
    state.isSlowConnection = isSlowConnection;
    
    // Adjust buffer settings based on platform
    if (isSlowConnection) {
        log('Slow connection detected, increasing buffer requirements', 'PLATFORM');
        config.MIN_BUFFER_SIZE *= 2;
        config.TARGET_BUFFER_SIZE *= 1.5;
    }
    
    if (mobile) {
        log('Mobile device detected, adjusting buffer settings', 'PLATFORM');
        
        // Mobile devices need larger buffers
        config.MIN_BUFFER_SIZE *= config.MOBILE_BUFFER_MULTIPLIER;
        config.TARGET_BUFFER_SIZE *= config.MOBILE_BUFFER_MULTIPLIER;
        
        if (iOS) {
            // iOS has specific playback issues that need even larger buffers
            log(`iOS ${iOSVersion} detected, using iOS-specific settings`, 'PLATFORM');
            config.MIN_BUFFER_SIZE *= config.IOS_BUFFER_MULTIPLIER;
            config.TARGET_BUFFER_SIZE *= config.IOS_BUFFER_MULTIPLIER;
        }
    }
    
    return { 
        isIOS: iOS, 
        iOSVersion: iOSVersion,
        isAndroid: isAndroid,
        androidVersion: androidVersion,
        isMobile: mobile,
        isChrome: isChrome,
        isSafari: isSafari,
        isFirefox: isFirefox,
        isEdge: isEdge,
        connectionType: connectionType,
        isSlowConnection: isSlowConnection
    };
}

// Enhanced logging with categories and timestamps
function log(message, category = 'INFO', isError = false) {
    const timestamp = new Date().toISOString().substr(11, 8);
    const logPrefix = `[${timestamp}] [${category}]`;
    
    if (isError) {
        console.error(`${logPrefix} ${message}`);
        
        // Track errors in state for diagnostics
        state.lastErrorMessage = message;
        state.lastErrorTime = Date.now();
        
        // Record error history (limit to last 10)
        state.errorHistory = state.errorHistory || [];
        state.errorHistory.push({
            time: Date.now(),
            category: category,
            message: message
        });
        
        if (state.errorHistory.length > 10) {
            state.errorHistory.shift();
        }
    } else if (state.debugMode) {
        // Normal logging
        console.log(`${logPrefix} ${message}`);
    }
}

// Improved status message display with fade effect
function showStatus(message, isError = false, autoHide = true) {
    log(`Status: ${message}`, 'UI', isError);
    
    if (!statusMessage) return;
    
    // Clear any existing hide timer
    if (state.statusHideTimer) {
        clearTimeout(state.statusHideTimer);
        state.statusHideTimer = null;
    }
    
    // Update message
    statusMessage.textContent = message;
    statusMessage.style.display = 'block';
    statusMessage.style.opacity = '1';
    statusMessage.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
    
    // Add highlight effect for important messages
    if (isError) {
        statusMessage.style.animation = 'pulse 1s';
        setTimeout(() => {
            statusMessage.style.animation = '';
        }, 1000);
    }
    
    if (!isError && autoHide) {
        // Set timer to hide status
        state.statusHideTimer = setTimeout(() => {
            // Fade out
            statusMessage.style.transition = 'opacity 0.5s ease-out';
            statusMessage.style.opacity = '0';
            
            // Hide after fade
            setTimeout(() => {
                statusMessage.style.display = 'none';
                statusMessage.style.transition = '';
            }, 500);
        }, 3000);
    }
}

// Format time in minutes:seconds
function formatTime(seconds) {
    if (!seconds && seconds !== 0) return '0:00';
    
    const totalSeconds = Math.floor(seconds);
    const minutes = Math.floor(totalSeconds / 60);
    const secs = totalSeconds % 60;
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

// Initialize UI elements when document is ready
document.addEventListener('DOMContentLoaded', function() {
    // Find all UI elements
    startBtn = document.getElementById('start-btn');
    muteBtn = document.getElementById('mute-btn');
    volumeControl = document.getElementById('volume');
    progressBar = document.getElementById('progress-bar');
    currentPosition = document.getElementById('current-position');
    currentDuration = document.getElementById('current-duration');
    currentTitle = document.getElementById('current-title');
    currentArtist = document.getElementById('current-artist');
    currentAlbum = document.getElementById('current-album');
    listenerCount = document.getElementById('listener-count');
    statusMessage = document.getElementById('status-message');
});

// Main initialization with improved error handling
function initPlayer() {
    try {
        // Detect platform and adjust settings
        detectPlatform();
        
        // Log initialization
        log(`Web Radio initializing (${state.isIOS ? 'iOS' : state.isAndroid ? 'Android' : state.isMobile ? 'Mobile' : 'Desktop'}, ${state.connectionType} connection)`, 'INIT');
        
        // Add CSS for status animations if needed
        if (!document.getElementById('radio-player-animations')) {
            const style = document.createElement('style');
            style.id = 'radio-player-animations';
            style.textContent = `
                @keyframes pulse {
                    0% { transform: scale(1); }
                    50% { transform: scale(1.03); }
                    100% { transform: scale(1); }
                }
            `;
            document.head.appendChild(style);
        }
        
        // Set up event listeners with error handling
        if (startBtn) {
            startBtn.addEventListener('click', function(e) {
                try {
                    toggleConnection();
                } catch (error) {
                    log(`Error in connection toggle: ${error.message}`, 'UI', true);
                    showStatus('Error connecting to stream', true);
                }
            });
        }
        
        if (muteBtn) {
            muteBtn.addEventListener('click', function() {
                state.isMuted = !state.isMuted;
                
                if (state.audioElement) {
                    state.audioElement.muted = state.isMuted;
                }
                
                muteBtn.textContent = state.isMuted ? 'Unmute' : 'Mute';
                
                // Save preference
                try {
                    localStorage.setItem('radioMuted', state.isMuted ? 'true' : 'false');
                } catch (e) {
                    // Ignore storage errors
                }
            });
        }
        
        if (volumeControl) {
            volumeControl.addEventListener('input', function() {
                if (state.audioElement) {
                    state.audioElement.volume = this.value;
                }
                
                try {
                    localStorage.setItem('radioVolume', this.value);
                } catch (e) {
                    // Ignore storage errors
                }
            });
            
            // Load saved volume and mute settings from localStorage
            try {
                const savedVolume = localStorage.getItem('radioVolume');
                if (savedVolume !== null) {
                    volumeControl.value = savedVolume;
                }
                
                const savedMuted = localStorage.getItem('radioMuted');
                if (savedMuted === 'true') {
                    state.isMuted = true;
                    if (muteBtn) muteBtn.textContent = 'Unmute';
                }
            } catch (e) {
                // Ignore storage errors
                log(`Error loading saved settings: ${e.message}`, 'INIT');
            }
        }
        
        // Fetch initial track info
        fetchNowPlaying().catch(error => {
            log(`Initial track info fetch failed: ${error.message}`, 'INIT', false);
            // Non-critical error, don't show to user
        });
        
        // Add window unload handler to clean up resources
        window.addEventListener('beforeunload', function() {
            // Clean up any audio elements when navigating away
            if (state.audioElement) {
                try {
                    state.audioElement.pause();
                    state.audioElement.src = '';
                    state.audioElement.load();
                } catch (e) {
                    // Ignore errors during cleanup
                }
            }
            
            // Clear all timers
            if (typeof clearAllTimers === 'function') {
                clearAllTimers();
            }
        });
        
        // Handle visibility changes for mobile optimization
        document.addEventListener('visibilitychange', function() {
            if (document.hidden) {
                log('Page hidden, suspending non-essential updates', 'VISIBILITY');
                // Pause non-essential updates when page is hidden
                if (state.bufferMonitorInterval) {
                    clearInterval(state.bufferMonitorInterval);
                    state.bufferMonitorInterval = null;
                }
            } else {
                log('Page visible, resuming updates', 'VISIBILITY');
                // Resume updates when page becomes visible again
                if (state.isPlaying && !state.bufferMonitorInterval) {
                    startEnhancedBufferMonitoring();
                }
            }
        });
        
        // Auto-start playback if requested in URL (e.g. ?autoplay=true)
        try {
            const urlParams = new URLSearchParams(window.location.search);
            if (urlParams.get('autoplay') === 'true' && startBtn) {
                log('Auto-play requested in URL', 'INIT');
                // Delay slightly to ensure page is fully loaded
                setTimeout(() => {
                    if (startBtn.dataset.connected !== 'true') {
                        startBtn.click();
                    }
                }, 1000);
            }
        } catch (e) {
            // Ignore URL parsing errors
        }
        
        log('Web Radio player successfully initialized', 'INIT');
    } catch (error) {
        log(`Error initializing player: ${error.message}`, 'INIT', true);
        showStatus('Error initializing player. Please refresh the page.', true, false);
    }
}

// Export functions for other modules
window.formatTime = formatTime;
window.log = log;
window.showStatus = showStatus;
window.detectPlatform = detectPlatform;
window.initPlayer = initPlayer;

// Entry point - will be called by player.js
// document.addEventListener('DOMContentLoaded', initPlayer);