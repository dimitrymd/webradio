// Updated player-core.js for direct streaming on all platforms

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
    
    // Direct streaming
    nowPlayingInterval: null,
    lastTrackChange: 0,
    trackPlaybackDuration: 0,
    
    // Performance monitoring
    bufferUnderflows: 0
};

// Configuration constants
const config = {
    NOW_PLAYING_INTERVAL: 5000,     // Check now playing every 5 seconds
    RETRY_DELAY: 2000,              // Delay between retry attempts
    MAX_RETRIES: 5,                 // Maximum retry attempts
    TRACK_CHECK_INTERVAL: 1000      // Check track position every second
};

// Enhanced platform detection function
function detectPlatform() {
    const ua = window.navigator.userAgent;
    
    // iOS detection
    const iOS = /iPad|iPhone|iPod/.test(ua) || 
                (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
    
    // General mobile detection
    const mobile = iOS || /Android|webOS|BlackBerry|IEMobile|Opera Mini/i.test(ua);
    
    // Store in state
    state.isIOS = iOS;
    state.isMobile = mobile;
    
    if (iOS) {
        log(`Detected iOS device: ${ua}`, 'PLATFORM');
    } else if (mobile) {
        log(`Detected mobile device: ${ua}`, 'PLATFORM');
    }
    
    return { isIOS: iOS, isMobile: mobile };
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