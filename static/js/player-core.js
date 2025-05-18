// Updated player-core.js with enhanced platform detection and state tracking

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
    // Audio elements
    audioElement: null,
    isPlaying: false,
    isMuted: false,
    reconnectAttempts: 0,
    maxReconnectAttempts: 5,
    lastErrorTime: 0,
    
    // Track info state
    currentTrackId: null,
    lastKnownPosition: 0,
    lastTrackInfoTime: 0,
    
    // Platform detection
    isIOS: false,
    isMobile: false,
    
    // Direct streaming state
    nowPlayingInterval: null,
    lastTrackChange: 0,
    trackPlaybackDuration: 0,
    
    // Position syncing
    serverPosition: 0,
    positionSyncAttempted: false,
    lastPositionSync: 0,
    
    // Performance monitoring
    bufferUnderflows: 0,
    
    // Debugging
    debugMode: true  // Set to true to enable detailed logging
};

// Configuration constants (expanded for better position syncing)
const config = {
    NOW_PLAYING_INTERVAL: 5000,     // Check now playing every 5 seconds
    RETRY_DELAY: 2000,              // Delay between retry attempts
    MAX_RETRIES: 5,                 // Maximum retry attempts
    TRACK_CHECK_INTERVAL: 1000,     // Check track position every second
    POSITION_SYNC_INTERVAL: 30000,  // Re-sync position with server every 30 seconds
    IOS_SEEK_RETRY_COUNT: 3,        // Number of times to retry seeking on iOS
    POSITION_SYNC_THRESHOLD: 10     // Seconds difference before considering position out of sync
};

// Enhanced platform detection function with iOS version detection
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
    }
    
    // General mobile detection
    const mobile = iOS || /Android|webOS|BlackBerry|IEMobile|Opera Mini/i.test(ua);
    
    // Chrome detection - useful for specific Chrome workarounds
    const isChrome = /Chrome/.test(ua) && /Google Inc/.test(navigator.vendor);
    
    // Store in state
    state.isIOS = iOS;
    state.iOSVersion = iOSVersion;
    state.isMobile = mobile;
    state.isChrome = isChrome;
    
    if (iOS) {
        log(`Detected iOS device (version ${iOSVersion || 'unknown'}): ${ua}`, 'PLATFORM');
    } else if (mobile) {
        log(`Detected mobile device: ${ua}`, 'PLATFORM');
    }
    
    return { 
        isIOS: iOS, 
        iOSVersion: iOSVersion,
        isMobile: mobile,
        isChrome: isChrome 
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
    if (isError) {
        const timestamp = new Date().toISOString().substr(11, 8);
        console.error(`[${timestamp}] [${category}] ${message}`);
    } else if (state.debugMode) {
        const timestamp = new Date().toISOString().substr(11, 8);
        console.log(`[${timestamp}] [${category}] ${message}`);
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
    
    log(`Web Radio player initialized (iOS: ${state.isIOS}, Mobile: ${state.isMobile})`, 'INIT');
}

// Export functions for other modules
window.formatTime = formatTime;
window.log = log;
window.showStatus = showStatus;
window.detectPlatform = detectPlatform;

// Entry point
document.addEventListener('DOMContentLoaded', initPlayer);