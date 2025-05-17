// static/js/player-core.js - Updated with improved buffering configuration

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

// Global state
const state = {
    // WebSocket and audio context
    ws: null,
    audioElement: null,
    mediaSource: null,
    sourceBuffer: null,
    audioQueue: [],
    isPlaying: false,
    isMuted: false,
    reconnectAttempts: 0,
    maxReconnectAttempts: 20, // Increased from 15
    connectionTimeout: null,
    lastAudioChunkTime: Date.now(),
    debugMode: false,

    // Track info state
    currentTrackId: null,
    lastKnownPosition: 0,
    connectionHealthTimer: null,
    lastErrorTime: 0,
    consecutiveErrors: 0,
    lastTrackInfoTime: 0,
    
    // Platform detection
    isIOS: false,
    isMobile: false,
    
    // Direct streaming
    usingDirectStream: false,
    nowPlayingInterval: null,
    
    // Performance monitoring
    bufferUnderflows: 0,
    lastBufferHealth: 0,
    performanceMetrics: {
        avgBufferSize: 0,
        bufferSamples: 0,
        lastBufferCheck: Date.now()
    }
};

// Configuration constants with improved buffering settings
const config = {
    TARGET_BUFFER_SIZE: 20,         // Increased from 10 to 20 seconds
    MIN_BUFFER_SIZE: 5,             // Increased from 3 to 5 seconds
    MAX_BUFFER_SIZE: 60,            // Increased from 30 to 60 seconds
    BUFFER_MONITOR_INTERVAL: 1000,  // Check buffer every 1 second (was 3000)
    NO_DATA_TIMEOUT: 30,            // Increased from 20 to 30 seconds
    AUDIO_STARVATION_THRESHOLD: 3,  // Increased from 2 to 3 seconds
    NOW_PLAYING_INTERVAL: 10000,    // Keep unchanged
    BUFFER_BOOST_DURATION: 2000,    // Time to boost buffer before starting playback
    RECONNECT_DELAY_BASE: 200,      // Base delay for reconnection (reduced)
    RECONNECT_BACKOFF_FACTOR: 1.2   // Lower backoff factor for faster reconnects
};

// Enhanced platform detection function
function detectPlatform() {
    const ua = window.navigator.userAgent;
    
    // More comprehensive iOS detection
    const iOS = /iPad|iPhone|iPod/.test(ua) || 
                (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
    
    // General mobile detection
    const mobile = iOS || /Android|webOS|BlackBerry|IEMobile|Opera Mini/i.test(ua);
    
    // Store in state
    state.isIOS = iOS;
    state.isMobile = mobile;
    
    if (iOS) {
        log(`Detected iOS device: ${ua}`, 'PLATFORM');
        log('iOS device detected - will use direct streaming instead of MSE', 'PLATFORM');
        
        // Enable debug mode for iOS to help troubleshoot
        state.debugMode = true;
    } else if (mobile) {
        log(`Detected mobile device: ${ua}`, 'PLATFORM');
    }
    
    return { isIOS: iOS, isMobile: mobile };
}

// Get the appropriate WebSocket URL based on platform
function getWebSocketURL() {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const host = window.location.host;
    return `${protocol}//${host}/stream`;
}

// Get MIME type for source buffer
function getSourceBufferType() {
    return 'audio/mpeg';
}

// Improved MSE compatibility check
function checkMSECompatibility() {
    if (!('MediaSource' in window)) {
        return {
            supported: false,
            message: 'MediaSource API not supported'
        };
    }
    
    const mimeType = getSourceBufferType();
    const isSupported = MediaSource.isTypeSupported(mimeType);
    
    return {
        supported: isSupported,
        message: isSupported ? 
            `MSE supports ${mimeType}` : 
            `MSE does not support ${mimeType}`
    };
}

// Utility functions
function formatTime(seconds) {
    if (!seconds) return '0:00';
    const minutes = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

function log(message, category = 'INFO', isError = false) {
    if (isError || state.debugMode) {
        const timestamp = new Date().toISOString().substr(11, 8);
        console[isError ? 'error' : 'log'](`[${timestamp}] [${category}] ${message}`);
    }
}

function showStatus(message, isError = false, autoHide = true) {
    log(`Status: ${message}`, 'UI', isError);
    
    if (!statusMessage) return;
    
    statusMessage.textContent = message;
    statusMessage.style.display = 'block';
    statusMessage.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
    
    if (!isError && autoHide) {
        setTimeout(() => {
            statusMessage.style.display = 'none';
        }, 3000);
    }
}

// New function to boost initial buffer for smoother playback
function boostInitialBuffer() {
    // Only execute on first connect
    if (state.reconnectAttempts === 0) {
        log('Boosting initial buffer size for smoother playback', 'BUFFER');
        
        // Pause audio briefly to build a bigger initial buffer
        if (state.audioElement && !state.audioElement.paused) {
            state.audioElement.pause();
            
            // Start a timer to check buffer level and resume when ready
            const checkBufferAndResume = () => {
                const bufferHealth = getBufferHealth();
                
                // Resume if buffer is good or max time elapsed
                if (bufferHealth.ahead >= config.MIN_BUFFER_SIZE * 1.5) {
                    log(`Initial buffer built to ${bufferHealth.ahead.toFixed(1)}s, resuming playback`, 'BUFFER');
                    state.audioElement.play().catch(e => {
                        log(`Error resuming playback: ${e.message}`, 'AUDIO', true);
                    });
                } else if (state.audioQueue.length === 0) {
                    // No data coming, resume anyway
                    log('No data received, resuming anyway', 'BUFFER');
                    state.audioElement.play().catch(e => {
                        log(`Error resuming playback: ${e.message}`, 'AUDIO', true);
                    });
                } else {
                    // Check again soon
                    setTimeout(checkBufferAndResume, 100);
                }
            };
            
            // Start checking after a brief delay
            setTimeout(checkBufferAndResume, 500);
        }
    }
}

// Optimize settings for mobile devices
function optimizeMobileSettings() {
    if (state.isMobile) {
        log('Applying mobile-specific optimizations', 'CONFIG');
        
        // Adjust config for mobile
        config.TARGET_BUFFER_SIZE = 15;        // Slightly less than desktop
        config.MIN_BUFFER_SIZE = 3;            // Lower threshold
        config.AUDIO_STARVATION_THRESHOLD = 2; // Be more aggressive in refilling
        
        // Set audio element properties for better mobile playback
        if (state.audioElement) {
            state.audioElement.preload = 'auto';
            
            // Add playback rate adjustment - play slightly slower to build buffer
            state.audioElement.playbackRate = 0.98; // 2% slower - imperceptible but helps buffering
        }
    }
}

// Main initialization
function initPlayer() {
    // Detect platform
    detectPlatform();
    
    // Set up event listeners
    if (startBtn) {
        startBtn.addEventListener('click', toggleConnection);
    }
    
    if (muteBtn) {
        muteBtn.addEventListener('click', function() {
            state.isMuted = !state.isMuted;
            
            if (state.audioElement) {
                state.audioElement.muted = state.isMuted;
            }
            
            muteBtn.textContent = state.isMuted ? 'Unmute' : 'Mute';
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
        
        // Load saved volume from localStorage
        try {
            const savedVolume = localStorage.getItem('radioVolume');
            if (savedVolume !== null) {
                volumeControl.value = savedVolume;
            }
        } catch (e) {
            // Ignore storage errors
        }
    }
    
    // Fetch initial track info
    fetchNowPlaying();
    
    // Apply mobile optimizations if needed
    optimizeMobileSettings();
    
    log(`Web Radio player initialized (iOS: ${state.isIOS}, Mobile: ${state.isMobile})`, 'INIT');
}

// Export functions for other modules
window.formatTime = formatTime;
window.log = log;
window.showStatus = showStatus;
window.getWebSocketURL = getWebSocketURL;
window.getSourceBufferType = getSourceBufferType;
window.checkMSECompatibility = checkMSECompatibility;
window.detectPlatform = detectPlatform;
window.boostInitialBuffer = boostInitialBuffer;
window.optimizeMobileSettings = optimizeMobileSettings;

// Entry point
document.addEventListener('DOMContentLoaded', initPlayer);