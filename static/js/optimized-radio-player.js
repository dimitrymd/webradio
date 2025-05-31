// static/js/optimized-radio-player.js - Simplified and Working

// Simple radio state
const radioState = {
    audioElement: null,
    isPlaying: false,
    isMuted: false,
    volume: 0.7,
    currentTrack: null,
    serverPosition: 0,
    isIOS: /iPad|iPhone|iPod/.test(navigator.userAgent),
    isMobile: /Mobi|Android/i.test(navigator.userAgent),
    
    // Timers
    nowPlayingTimer: null,
    heartbeatTimer: null,
    
    // Connection
    connectionId: null,
    consecutiveErrors: 0,
    isReconnecting: false
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

// Initialize the radio player
function initRadioPlayer() {
    console.log('🎵 ChillOut Radio - Initializing...');
    
    // Check UI elements
    if (!startBtn || !muteBtn || !volumeControl) {
        console.error('❌ Required UI elements not found');
        return;
    }
    
    // Set up event listeners
    setupEventListeners();
    
    // Load saved settings
    loadSettings();
    
    // Start fetching track info
    startTrackInfoUpdates();
    
    // Initial fetch
    fetchNowPlaying();
    
    console.log('✅ Radio player initialized');
    showStatus('📻 Radio ready - click "Tune In" to start listening');
}

// Set up event listeners
function setupEventListeners() {
    startBtn.addEventListener('click', toggleRadio);
    muteBtn.addEventListener('click', toggleMute);
    volumeControl.addEventListener('input', (e) => {
        updateVolume(parseFloat(e.target.value));
    });
}

// Toggle radio on/off
function toggleRadio() {
    if (radioState.isPlaying) {
        stopRadio();
    } else {
        startRadio();
    }
}

// Start radio
function startRadio() {
    console.log('🎵 Starting radio...');
    
    if (radioState.isPlaying || radioState.isReconnecting) {
        console.log('Already playing or reconnecting');
        return;
    }
    
    radioState.isPlaying = true;
    startBtn.disabled = true;
    showStatus('📻 Connecting to radio stream...');
    
    // Create audio element
    createAudioElement();
    
    // Start streaming
    startStreaming();
    
    // Update UI
    startBtn.textContent = '📻 Disconnect';
    startBtn.dataset.connected = 'true';
}

// Create audio element
function createAudioElement() {
    // Clean up existing element
    if (radioState.audioElement) {
        radioState.audioElement.pause();
        radioState.audioElement.src = '';
        radioState.audioElement = null;
    }
    
    console.log('🔊 Creating audio element...');
    
    radioState.audioElement = new Audio();
    radioState.audioElement.volume = radioState.volume;
    radioState.audioElement.muted = radioState.isMuted;
    radioState.audioElement.crossOrigin = "anonymous";
    
    // Platform-specific settings
    if (radioState.isIOS) {
        radioState.audioElement.preload = 'none';
        radioState.audioElement.playsInline = true;
    } else {
        radioState.audioElement.preload = 'metadata';
    }
    
    // Set up event listeners
    setupAudioEventListeners();
}

// Set up audio event listeners
function setupAudioEventListeners() {
    const audio = radioState.audioElement;
    
    audio.addEventListener('loadstart', () => {
        console.log('🔄 Loading stream...');
        showStatus('📻 Loading radio stream...');
    });
    
    audio.addEventListener('canplay', () => {
        console.log('✅ Stream ready to play');
        showStatus('📻 Stream ready...');
    });
    
    audio.addEventListener('playing', () => {
        console.log('▶️ Radio playing');
        showStatus('📻 Tuned in to ChillOut Radio');
        startBtn.disabled = false;
        radioState.consecutiveErrors = 0;
    });
    
    audio.addEventListener('waiting', () => {
        console.log('⏳ Buffering...');
        showStatus('📻 Buffering...');
    });
    
    audio.addEventListener('stalled', () => {
        console.log('⚠️ Stream stalled');
        showStatus('📻 Connection issues - buffering...');
    });
    
    audio.addEventListener('error', (e) => {
        const error = e.target.error;
        const errorMsg = getErrorMessage(error);
        console.error('❌ Audio error:', errorMsg);
        
        radioState.consecutiveErrors++;
        
        if (error && error.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED) {
            showStatus('📻 Audio format not supported', true);
        } else if (error && error.code === MediaError.MEDIA_ERR_NETWORK) {
            showStatus('📻 Network error - will retry...', true);
            scheduleReconnect();
        } else {
            showStatus(`📻 Error: ${errorMsg}`, true);
            if (radioState.consecutiveErrors < 3) {
                scheduleReconnect();
            } else {
                stopRadio(true);
            }
        }
    });
    
    audio.addEventListener('ended', () => {
        console.log('🔄 Stream ended - reconnecting...');
        scheduleReconnect();
    });
}

// Start streaming
function startStreaming() {
    if (!radioState.audioElement) {
        console.error('❌ No audio element');
        return;
    }
    
    // Create stream URL
    const timestamp = Date.now();
    let streamUrl = `/direct-stream?t=${timestamp}`;
    
    // Add platform info
    if (radioState.isIOS) {
        streamUrl += '&platform=ios';
    } else if (radioState.isMobile) {
        streamUrl += '&platform=mobile';
    } else {
        streamUrl += '&platform=desktop';
    }
    
    console.log('🌐 Stream URL:', streamUrl);
    
    // Set source and play
    radioState.audioElement.src = streamUrl;
    
    // Attempt to play
    const playPromise = radioState.audioElement.play();
    if (playPromise !== undefined) {
        playPromise.then(() => {
            console.log('✅ Playback started');
        }).catch(error => {
            console.error('❌ Playback failed:', error);
            if (error.name === 'NotAllowedError') {
                showStatus('📻 Click to enable audio playback', true);
                startBtn.textContent = '🔊 Enable Audio';
                startBtn.disabled = false;
                startBtn.onclick = () => {
                    radioState.audioElement.play().then(() => {
                        startBtn.onclick = toggleRadio;
                        startBtn.textContent = '📻 Disconnect';
                    });
                };
            } else {
                scheduleReconnect();
            }
        });
    }
}

// Stop radio
function stopRadio(isError = false) {
    console.log('⏹️ Stopping radio...');
    
    radioState.isPlaying = false;
    radioState.isReconnecting = false;
    
    // Stop audio
    if (radioState.audioElement) {
        radioState.audioElement.pause();
        radioState.audioElement.src = '';
        radioState.audioElement = null;
    }
    
    // Clear timers
    clearTimers();
    
    // Update UI
    startBtn.textContent = '📻 Tune In';
    startBtn.disabled = false;
    startBtn.dataset.connected = 'false';
    startBtn.onclick = toggleRadio;
    
    if (!isError) {
        showStatus('📻 Disconnected from radio');
    }
}

// Schedule reconnect
function scheduleReconnect() {
    if (radioState.isReconnecting || !radioState.isPlaying) {
        return;
    }
    
    radioState.isReconnecting = true;
    console.log('🔄 Scheduling reconnect...');
    
    const delay = Math.min(2000 * Math.pow(2, radioState.consecutiveErrors), 10000);
    showStatus(`📻 Reconnecting in ${Math.round(delay/1000)}s...`);
    
    setTimeout(() => {
        if (radioState.isPlaying) {
            console.log('🔄 Attempting reconnect...');
            radioState.isReconnecting = false;
            createAudioElement();
            startStreaming();
        }
    }, delay);
}

// Toggle mute
function toggleMute() {
    radioState.isMuted = !radioState.isMuted;
    
    if (radioState.audioElement) {
        radioState.audioElement.muted = radioState.isMuted;
    }
    
    muteBtn.textContent = radioState.isMuted ? '🔇 Unmute' : '🔊 Mute';
    
    // Save setting
    try {
        localStorage.setItem('radioMuted', radioState.isMuted.toString());
    } catch (e) {
        // Ignore storage errors
    }
}

// Update volume
function updateVolume(volume) {
    radioState.volume = volume;
    
    if (radioState.audioElement) {
        radioState.audioElement.volume = volume;
    }
    
    // Save setting
    try {
        localStorage.setItem('radioVolume', volume.toString());
    } catch (e) {
        // Ignore storage errors
    }
}

// Load settings
function loadSettings() {
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
        console.log('Could not load settings');
    }
}

// Start track info updates
function startTrackInfoUpdates() {
    // Clear existing timers
    clearTimers();
    
    // Fetch now playing every 8 seconds
    radioState.nowPlayingTimer = setInterval(fetchNowPlaying, 8000);
    
    // Send heartbeat every 15 seconds
    radioState.heartbeatTimer = setInterval(sendHeartbeat, 15000);
}

// Clear timers
function clearTimers() {
    if (radioState.nowPlayingTimer) {
        clearInterval(radioState.nowPlayingTimer);
        radioState.nowPlayingTimer = null;
    }
    if (radioState.heartbeatTimer) {
        clearInterval(radioState.heartbeatTimer);
        radioState.heartbeatTimer = null;
    }
}

// Fetch now playing
async function fetchNowPlaying() {
    try {
        const response = await fetch('/api/now-playing', {
            headers: {
                'Cache-Control': 'no-cache'
            }
        });
        
        if (!response.ok) {
            console.error('❌ Now playing API error:', response.status);
            return;
        }
        
        const data = await response.json();
        updateTrackInfo(data);
        
    } catch (error) {
        console.error('❌ Error fetching now playing:', error);
    }
}

// Update track info
function updateTrackInfo(info) {
    try {
        if (info.error) {
            console.error('❌ Server error:', info.error);
            return;
        }
        
        radioState.currentTrack = info;
        radioState.serverPosition = info.radio_position || info.playback_position || 0;
        
        // Update UI
        if (currentTitle) currentTitle.textContent = info.title || 'Unknown Title';
        if (currentArtist) currentArtist.textContent = info.artist || 'Unknown Artist';
        if (currentAlbum) currentAlbum.textContent = info.album || 'Unknown Album';
        
        // Update duration
        if (currentDuration && info.duration) {
            currentDuration.textContent = formatTime(info.duration);
        }
        
        // Update position
        if (currentPosition) {
            currentPosition.textContent = formatTime(radioState.serverPosition);
        }
        
        // Update progress bar
        if (progressBar && info.duration && info.duration > 0) {
            const percent = (radioState.serverPosition / info.duration) * 100;
            progressBar.style.width = `${Math.min(100, Math.max(0, percent))}%`;
        }
        
        // Update listener count
        if (listenerCount && info.active_listeners !== undefined) {
            listenerCount.innerHTML = `<span class="radio-live">LIVE</span> • Listeners: ${info.active_listeners}`;
        }
        
        // Update document title
        if (info.title && info.artist) {
            document.title = `📻 ${info.title} - ${info.artist} | ChillOut Radio`;
        }
        
    } catch (error) {
        console.error('❌ Error updating track info:', error);
    }
}

// Send heartbeat
async function sendHeartbeat() {
    if (!radioState.isPlaying || !radioState.connectionId) {
        return;
    }
    
    try {
        const response = await fetch(`/api/heartbeat?connection_id=${radioState.connectionId}`, {
            headers: {
                'Cache-Control': 'no-cache'
            }
        });
        
        if (response.ok) {
            const data = await response.json();
            
            // Update listener count from heartbeat
            if (data.active_listeners !== undefined && listenerCount) {
                listenerCount.innerHTML = `<span class="radio-live">LIVE</span> • Listeners: ${data.active_listeners}`;
            }
        }
    } catch (error) {
        console.log('Heartbeat failed:', error);
    }
}

// Show status message
function showStatus(message, isError = false, autoHide = true) {
    console.log(`Status: ${message}`);
    
    if (statusMessage) {
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

// Format time
function formatTime(seconds) {
    if (!seconds || seconds < 0) return '0:00';
    const minutes = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

// Initialize when page loads
document.addEventListener('DOMContentLoaded', () => {
    try {
        initRadioPlayer();
    } catch (error) {
        console.error('❌ Failed to initialize radio player:', error);
        alert('Radio player failed to initialize');
    }
});

// Cleanup on page unload
window.addEventListener('beforeunload', () => {
    clearTimers();
    if (radioState.audioElement) {
        radioState.audioElement.pause();
        radioState.audioElement.src = '';
    }
});

// Debug object
window.ChillOutRadio = {
    state: radioState,
    start: startRadio,
    stop: stopRadio,
    fetchInfo: fetchNowPlaying,
    version: '2.4.0-simplified'
};

console.log('🎵 ChillOut Radio v2.4.0 - Simplified Player Loaded');
console.log('Debug: window.ChillOutRadio available'); 