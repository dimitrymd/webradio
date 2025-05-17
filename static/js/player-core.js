// static/js/player-core.js - Complete file

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
    maxReconnectAttempts: 15,
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
    
    // Direct streaming
    usingDirectStream: false,
    nowPlayingInterval: null
};

// Configuration constants
const config = {
    TARGET_BUFFER_SIZE: 10,         // Target buffer duration in seconds
    MIN_BUFFER_SIZE: 3,             // Minimum buffer before playback starts
    MAX_BUFFER_SIZE: 30,            // Maximum buffer size in seconds
    BUFFER_MONITOR_INTERVAL: 3000,  // Check buffer every 3 seconds
    NO_DATA_TIMEOUT: 20,            // Timeout for no data in seconds
    AUDIO_STARVATION_THRESHOLD: 2,  // Seconds of buffer left before action needed
    NOW_PLAYING_INTERVAL: 10000     // Check now playing every 10 seconds
};

// Enhanced platform detection function
function detectIOSPlatform() {
    const ua = window.navigator.userAgent;
    
    // More comprehensive iOS detection
    const iOS = /iPad|iPhone|iPod/.test(ua) || 
                (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
    
    // Store in state
    state.isIOS = iOS;
    
    if (iOS) {
        log(`Detected iOS device: ${ua}`, 'PLATFORM');
        log('iOS device detected - will use direct streaming instead of MSE', 'PLATFORM');
        
        // Enable debug mode for iOS to help troubleshoot
        state.debugMode = true;
    }
    
    return iOS;
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

// Check MSE compatibility
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

// Main initialization
function initPlayer() {
    // Detect iOS platform
    detectIOSPlatform();
    
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
    
    log(`Web Radio player initialized (iOS: ${state.isIOS})`, 'INIT');
}

// Export functions for other modules
window.formatTime = formatTime;
window.log = log;
window.showStatus = showStatus;
window.getWebSocketURL = getWebSocketURL;
window.getSourceBufferType = getSourceBufferType;
window.checkMSECompatibility = checkMSECompatibility;
window.detectIOSPlatform = detectIOSPlatform;

// Entry point
document.addEventListener('DOMContentLoaded', initPlayer);