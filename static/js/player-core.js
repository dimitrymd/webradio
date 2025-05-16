// player-core.js update - Add iOS detection and configuration

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
    isIOSChrome: false
};

// Configuration constants
const config = {
    TARGET_BUFFER_SIZE: 10,         // Target buffer duration in seconds
    MIN_BUFFER_SIZE: 3,             // Minimum buffer before playback starts
    MAX_BUFFER_SIZE: 30,            // Maximum buffer size in seconds
    BUFFER_MONITOR_INTERVAL: 3000,  // Check buffer every 3 seconds
    NO_DATA_TIMEOUT: 20,            // Timeout for no data in seconds
    AUDIO_STARVATION_THRESHOLD: 2,  // Seconds of buffer left before action needed
    NOW_PLAYING_INTERVAL: 10000     // Check now playing every 10 seconds (changed from 2s)
};

// Detect iOS platform
function detectIOSPlatform() {
    const ua = window.navigator.userAgent;
    const iOS = /iPad|iPhone|iPod/.test(ua) || 
                (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
    const isIOSChrome = iOS && /CriOS/.test(ua);
    
    state.isIOS = iOS;
    state.isIOSChrome = isIOSChrome;
    
    if (iOS) {
        log(`Detected iOS device: ${ua}`, 'PLATFORM');
        
        // Additional iOS-specific settings
        config.MIN_BUFFER_SIZE = 5;  // Increase minimum buffer for iOS
        config.TARGET_BUFFER_SIZE = 15;  // Increase target buffer for iOS
        
        if (isIOSChrome) {
            log('Using Chrome on iOS', 'PLATFORM');
        } else {
            log('Using Safari on iOS', 'PLATFORM');
        }
    }
}

// Get the appropriate WebSocket URL based on platform
function getWebSocketURL() {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const host = window.location.host;
    
    // Use Opus stream for iOS devices
    if (state.isIOS) {
        return `${protocol}//${host}/stream-opus`;
    } else {
        return `${protocol}//${host}/stream`;
    }
}

// Get MIME type for source buffer based on platform
function getSourceBufferType() {
    if (state.isIOS) {
        return 'audio/ogg; codecs=opus';
    } else {
        return 'audio/mpeg';
    }
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
    
    // Update UI if on iOS
    if (state.isIOS) {
        // Add iOS indicator to player UI
        const playerEl = document.querySelector('.player');
        const iosIndicator = document.createElement('div');
        iosIndicator.className = 'ios-indicator';
        iosIndicator.textContent = 'iOS Mode (Opus Stream)';
        iosIndicator.style.textAlign = 'center';
        iosIndicator.style.padding = '5px';
        iosIndicator.style.margin = '5px 0';
        iosIndicator.style.backgroundColor = '#e8f7ff';
        iosIndicator.style.color = '#0066cc';
        iosIndicator.style.borderRadius = '5px';
        iosIndicator.style.fontSize = '14px';
        
        // Insert after player title
        const logo = playerEl.querySelector('.logo');
        if (logo && logo.nextSibling) {
            playerEl.insertBefore(iosIndicator, logo.nextSibling);
        } else {
            playerEl.appendChild(iosIndicator);
        }
    }
    
    // Set up event listeners
    startBtn.addEventListener('click', toggleConnection);
    
    muteBtn.addEventListener('click', function() {
        state.isMuted = !state.isMuted;
        
        if (state.audioElement) {
            state.audioElement.muted = state.isMuted;
        }
        
        muteBtn.textContent = state.isMuted ? 'Unmute' : 'Mute';
    });
    
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
    
    // Fetch initial track info
    fetchNowPlaying();
    
    log(`ChillOut Radio player initialized (iOS: ${state.isIOS})`, 'INIT');
}

// Entry point
document.addEventListener('DOMContentLoaded', initPlayer);