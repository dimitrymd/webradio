// universal-player.js - Simple, reliable audio player for all browsers including iOS
// Configuration constants
const config = {
    // Playback settings
    NOW_PLAYING_INTERVAL: 10000,     // Check now playing every 10 seconds
    DEBUG_MODE: true                 // Enable for verbose logging
};

// Player state
const state = {
    // Audio element 
    audioElement: null,
    
    // Connection and status
    isPlaying: false,
    isMuted: false,
    volume: 0.7,
    lastTrackInfoTime: Date.now(),
    
    // Track info
    currentTrackId: null,
    currentTrack: null,
    serverPosition: 0,
    
    // Timers
    nowPlayingTimer: null,
    
    // Platform detection
    isIOS: /iPad|iPhone|iPod/.test(navigator.userAgent) && !window.MSStream,
    isSafari: /^((?!chrome|android).)*safari/i.test(navigator.userAgent),
    isMobile: /Mobi|Android/i.test(navigator.userAgent)
};

// UI Elements
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

// Initialize the player
function initPlayer() {
    log("Initializing direct streaming radio player");
    log(`Detected platform - iOS: ${state.isIOS}, Safari: ${state.isSafari}, Mobile: ${state.isMobile}`);
    
    // Set up event listeners
    startBtn.addEventListener('click', toggleConnection);
    
    muteBtn.addEventListener('click', function() {
        state.isMuted = !state.isMuted;
        
        if (state.audioElement) {
            state.audioElement.muted = state.isMuted;
        }
        
        muteBtn.textContent = state.isMuted ? 'Unmute' : 'Mute';
        
        try {
            localStorage.setItem('radioMuted', state.isMuted.toString());
        } catch (e) {
            // Ignore storage errors
        }
    });
    
    volumeControl.addEventListener('input', function() {
        state.volume = this.value;
        
        if (state.audioElement) {
            state.audioElement.volume = state.volume;
        }
        
        try {
            localStorage.setItem('radioVolume', this.value);
        } catch (e) {
            // Ignore storage errors
        }
    });
    
    // Load saved volume and mute state from localStorage
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
        // Ignore storage errors
    }
    
    // Fetch initial track info
    fetchNowPlaying();
    
    log('ChillOut Radio player initialized');
    showStatus('Player ready - click Connect to start streaming', false, false);
}

// Start audio playback
function startAudio() {
    log('Starting audio playback', 'CONTROL');
    startBtn.disabled = true;
    showStatus('Connecting to stream...', false, false);
    
    // Reset state
    state.isPlaying = true;
    
    // Clean up existing audio element
    if (state.audioElement) {
        state.audioElement.pause();
        state.audioElement.src = '';
        state.audioElement.load();
        state.audioElement.remove();
    }
    
    // Create new audio element
    state.audioElement = new Audio();
    state.audioElement.controls = false;
    state.audioElement.volume = state.volume;
    state.audioElement.muted = state.isMuted;
    state.audioElement.preload = 'auto';
    state.audioElement.crossOrigin = "anonymous"; // For CORS if needed
    
    // Set up basic audio event listeners
    state.audioElement.addEventListener('playing', () => {
        log('Audio playing', 'AUDIO');
        showStatus('Stream playing');
    });
    
    state.audioElement.addEventListener('waiting', () => {
        log('Audio buffering', 'AUDIO');
        showStatus('Buffering...', false, false);
    });
    
    state.audioElement.addEventListener('canplaythrough', () => {
        log('Audio can play through', 'AUDIO');
        showStatus('Stream ready', false, true);
    });
    
    state.audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        log(`Audio error (code ${errorCode})`, 'AUDIO', true);
        showStatus(`Audio error - please try again`, true);
        startBtn.disabled = false;
    });
    
    // Progress update for time display
    state.audioElement.addEventListener('timeupdate', () => {
        if (state.audioElement && state.currentTrack && state.currentTrack.duration) {
            updateProgressBar(state.audioElement.currentTime, state.currentTrack.duration);
        }
    });
    
    // Add to document but hide visually
    state.audioElement.style.display = 'none';
    document.body.appendChild(state.audioElement);
    
    // Start direct streaming
    startDirectPlayback();
    
    // Set up now playing update timer
    if (state.nowPlayingTimer) {
        clearInterval(state.nowPlayingTimer);
    }
    state.nowPlayingTimer = setInterval(fetchNowPlaying, config.NOW_PLAYING_INTERVAL);
}

// Stop audio playback
function stopAudio(isError = false) {
    log(`Stopping audio playback${isError ? ' (due to error)' : ''}`, 'CONTROL');
    
    // Set state flag first
    state.isPlaying = false;
    
    // Clear all timers
    if (state.nowPlayingTimer) {
        clearInterval(state.nowPlayingTimer);
        state.nowPlayingTimer = null;
    }
    
    // Clean up audio element
    if (state.audioElement) {
        state.audioElement.pause();
        state.audioElement.src = '';
        state.audioElement.load();
        if (state.audioElement.parentNode) {
            state.audioElement.remove();
        }
        state.audioElement = null;
    }
    
    if (!isError) {
        showStatus('Disconnected from audio stream');
    }
    
    // Reset UI
    startBtn.textContent = 'Connect';
    startBtn.disabled = false;
    startBtn.dataset.connected = 'false';
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

// Start direct HTTP streaming
function startDirectPlayback() {
    try {
        // Get current position
        const position = state.serverPosition || 0;
        
        // Build URL with parameters
        const timestamp = Date.now();
        let streamUrl = `/direct-stream?position=${position}`;
        
        log(`Using stream URL: ${streamUrl}`, 'PLAYBACK');
        
        // Set the audio source
        if (state.audioElement) {
            state.audioElement.src = streamUrl;
            state.audioElement.load();
        } else {
            log("Audio element is null, can't set source", 'PLAYBACK', true);
            return;
        }
        
        // For iOS, handle special case
        if (state.isIOS) {
            // iOS requires user interaction to start playback
            showStatus('Ready - Tap play to start streaming', false, false);
            startBtn.textContent = 'Play';
            startBtn.disabled = false;
            startBtn.dataset.connected = 'true';
            
            // Setup click handler for iOS
            startBtn.onclick = function() {
                if (state.audioElement && state.audioElement.paused) {
                    startBtn.disabled = true;
                    showStatus('Starting playback...', false, false);
                    
                    // Add a short delay for iOS
                    setTimeout(() => {
                        if (!state.audioElement) return;
                        
                        state.audioElement.play().then(() => {
                            showStatus('Stream playing');
                            startBtn.textContent = 'Disconnect';
                            startBtn.disabled = false;
                        }).catch(e => {
                            log(`iOS play failed: ${e.message}`, 'AUDIO', true);
                            showStatus(`Playback error: ${e.message}`, true);
                            startBtn.disabled = false;
                        });
                    }, 100);
                    
                    // Reset onclick to normal toggle behavior after initial play
                    startBtn.onclick = toggleConnection;
                } else {
                    stopAudio();
                }
            };
        } else {
            // For other browsers, play automatically
            showStatus('Starting playback...', false, false);
            
            // Slight delay to ensure audio element is ready
            setTimeout(() => {
                if (!state.audioElement || !state.isPlaying) return;
                
                try {
                    const playPromise = state.audioElement.play();
                    playPromise.then(() => {
                        log('Direct stream playback started', 'AUDIO');
                        showStatus('Connected to stream');
                        startBtn.textContent = 'Disconnect';
                        startBtn.disabled = false;
                        startBtn.dataset.connected = 'true';
                    }).catch(e => {
                        log(`Direct stream playback error: ${e.message}`, 'AUDIO', true);
                        
                        if (e.name === 'NotAllowedError') {
                            // Browser requires user interaction
                            showStatus('Click play to start audio (browser requires user interaction)', true, false);
                            startBtn.disabled = false;
                            startBtn.dataset.connected = 'true';
                            
                            // Setup click handler for playback
                            startBtn.onclick = function() {
                                if (state.audioElement && state.audioElement.paused) {
                                    startBtn.disabled = true;
                                    state.audioElement.play().then(() => {
                                        showStatus('Stream playing');
                                        startBtn.textContent = 'Disconnect';
                                        startBtn.disabled = false;
                                    }).catch(e => {
                                        log(`Play failed: ${e.message}`, 'AUDIO', true);
                                        showStatus(`Playback error: ${e.message}`, true);
                                        startBtn.disabled = false;
                                    });
                                    // Reset onclick to normal toggle behavior after initial play
                                    startBtn.onclick = toggleConnection;
                                } else {
                                    stopAudio();
                                }
                            };
                        } else {
                            showStatus(`Playback error: ${e.message}`, true);
                            startBtn.disabled = false;
                        }
                    });
                } catch (directError) {
                    log(`Error starting playback: ${directError.message}`, 'AUDIO', true);
                    showStatus(`Playback error: ${directError.message}`, true);
                    startBtn.disabled = false;
                }
            }, 200);
        }
    } catch (e) {
        log(`Direct streaming error: ${e.message}`, 'AUDIO', true);
        showStatus(`Streaming error: ${e.message}`, true);
        stopAudio(true);
    }
}

// Update track info from API
function updateTrackInfo(info) {
    try {
        // Check for error message
        if (info.error) {
            showStatus(`Server error: ${info.error}`, true);
            return;
        }
        
        // Store current track info
        state.currentTrack = info;
        
        // Store server position for sync
        if (info.playback_position !== undefined) {
            state.serverPosition = info.playback_position;
            state.lastTrackInfoTime = Date.now();
        }
        
        // Store track ID for change detection
        const newTrackId = info.path;
        if (state.currentTrackId !== newTrackId) {
            log(`Track changed to: ${info.title}`, 'TRACK');
            state.currentTrackId = newTrackId;
            
            // If we're playing, we need to reconnect to get the new track
            if (state.isPlaying && state.audioElement) {
                log("Track changed, reloading stream with new track", 'TRACK');
                startDirectPlayback(); // This will use the latest server position
            }
        }
        
        // Update UI
        currentTitle.textContent = info.title || 'Unknown Title';
        currentArtist.textContent = info.artist || 'Unknown Artist';
        currentAlbum.textContent = info.album || 'Unknown Album';
        
        // Update progress
        if (info.duration) {
            currentDuration.textContent = formatTime(info.duration);
        }
        
        // Update progress bar if we have position
        if (info.playback_position !== undefined && info.duration) {
            updateProgressBar(info.playback_position, info.duration);
        }
        
        // Update listener count
        if (info.active_listeners !== undefined) {
            listenerCount.textContent = `Listeners: ${info.active_listeners}`;
        }
        
        // Update page title
        document.title = `${info.title} - ${info.artist} | ChillOut Radio`;
    } catch (e) {
        log(`Error processing track info: ${e.message}`, 'TRACK', true);
    }
}

// Fetch now playing info via API
async function fetchNowPlaying() {
    try {
        log("Fetching now playing information", 'API');
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            log(`Now playing API error: ${response.status}`, 'API', true);
            return null;
        }
        
        const data = await response.json();
        if (config.DEBUG_MODE) {
            log(`Received now playing data: ${JSON.stringify(data)}`, 'API');
        } else {
            log(`Received now playing data for: ${data.title || 'unknown track'}`, 'API');
        }
        
        updateTrackInfo(data);
        return data;
    } catch (error) {
        log(`Error fetching now playing: ${error.message}`, 'API', true);
        return null;
    }
}

// Update the progress bar
function updateProgressBar(position, duration) {
    if (progressBar && duration > 0) {
        const percent = (position / duration) * 100;
        progressBar.style.width = `${percent}%`;
        
        // Update text display
        if (currentPosition) currentPosition.textContent = formatTime(position);
        if (currentDuration) currentDuration.textContent = formatTime(duration);
    }
}

// Format time in mm:ss format
function formatTime(seconds) {
    if (!seconds) return '0:00';
    const minutes = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

// Show status message
function showStatus(message, isError = false, autoHide = true) {
    if (config.DEBUG_MODE) {
        log(`Status: ${message}`, 'UI', isError);
    }
    
    statusMessage.textContent = message;
    statusMessage.style.display = 'block';
    statusMessage.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
    
    if (!isError && autoHide) {
        setTimeout(() => {
            // Only hide if this is still the current message
            if (statusMessage.textContent === message) {
                statusMessage.style.display = 'none';
            }
        }, 3000);
    }
}

// Logging function with better formatting
function log(message, category = 'INFO', isError = false) {
    // Always log errors, but only log non-errors in debug mode
    if (isError || config.DEBUG_MODE) {
        const timestamp = new Date().toISOString().substr(11, 8);
        console[isError ? 'error' : 'log'](`[${timestamp}] [${category}] ${message}`);
    }
}

// Initialize the player on document ready
document.addEventListener('DOMContentLoaded', initPlayer);