// universal-player.js - Cross-browser compatible audio player
// This player is designed to work across all browsers, including mobile devices

// Configuration constants
const config = {
    // Playback settings
    NOW_PLAYING_INTERVAL: 10000,     // Check now playing every 10 seconds
    CONNECTION_CHECK_INTERVAL: 5000, // Check connection health every 5 seconds
    RECONNECT_ATTEMPTS: 10,          // Maximum reconnection attempts
    DEBUG_MODE: false                // Enable for verbose logging
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
    reconnectAttempts: 0,
    
    // Track info
    currentTrackId: null,
    currentTrack: null,
    serverPosition: 0,
    
    // Timers
    nowPlayingTimer: null,
    connectionHealthTimer: null,
    
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
    log("Initializing radio player");
    log(`Platform: ${state.isMobile ? 'Mobile' : 'Desktop'}, iOS: ${state.isIOS}, Safari: ${state.isSafari}`);
    
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
    
    log('Radio player initialized');
    showStatus('Player ready - click Connect to start streaming', false, false);
}

// Start audio playback
function startAudio() {
    log('Starting audio playback', 'CONTROL');
    startBtn.disabled = true;
    showStatus('Connecting to stream...', false, false);
    
    // Reset state
    state.isPlaying = true;
    state.reconnectAttempts = 0;
    
    // Clean up existing audio element
    cleanupAudioElement();
    
    // Create new audio element
    state.audioElement = new Audio();
    state.audioElement.controls = false;
    state.audioElement.volume = state.volume;
    state.audioElement.muted = state.isMuted;
    state.audioElement.preload = 'auto';
    state.audioElement.crossOrigin = "anonymous"; // For CORS if needed
    
    // Set up audio event listeners
    setupAudioListeners();
    
    // Start direct streaming
    startDirectPlayback();
    
    // Set up now playing update timer
    if (state.nowPlayingTimer) {
        clearInterval(state.nowPlayingTimer);
    }
    state.nowPlayingTimer = setInterval(fetchNowPlaying, config.NOW_PLAYING_INTERVAL);
    
    // Set up connection health check
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
    }
    state.connectionHealthTimer = setInterval(checkConnectionHealth, config.CONNECTION_CHECK_INTERVAL);
}

// Setup audio event listeners
function setupAudioListeners() {
    // Playback state events
    state.audioElement.addEventListener('playing', () => {
        log('Audio playing', 'AUDIO');
        showStatus('Audio playing');
    });
    
    state.audioElement.addEventListener('waiting', () => {
        log('Audio buffering', 'AUDIO');
        showStatus('Buffering...', false, false);
    });
    
    state.audioElement.addEventListener('stalled', () => {
        log('Audio stalled', 'AUDIO');
        showStatus('Stream stalled - buffering', true, false);
    });
    
    state.audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        log(`Audio error (code ${errorCode})`, 'AUDIO', true);
        
        if (state.isPlaying) {
            // Don't react to errors too frequently
            const now = Date.now();
            if (now - state.lastErrorTime > 10000) {
                state.lastErrorTime = now;
                showStatus(`Audio error - will try to reconnect`, true, false);
                
                // Schedule a reconnection attempt
                setTimeout(() => {
                    if (state.isPlaying) {
                        attemptReconnection();
                    }
                }, 2000);
            }
        }
    });
    
    state.audioElement.addEventListener('ended', () => {
        log('Audio ended', 'AUDIO');
        // If we shouldn't be at the end, try to restart
        if (state.isPlaying) {
            log('Audio ended unexpectedly, attempting to recover', 'AUDIO', true);
            showStatus('Audio ended - reconnecting', true, false);
            attemptReconnection();
        }
    });
    
    // Progress monitoring
    state.audioElement.addEventListener('timeupdate', () => {
        if (state.audioElement && state.currentTrack && state.currentTrack.duration) {
            updateProgressBar(state.audioElement.currentTime, state.currentTrack.duration);
        }
    });
}

// Clean up audio element properly
function cleanupAudioElement() {
    if (state.audioElement) {
        try {
            // First pause
            state.audioElement.pause();
            
            // Then clear source and load to flush the buffer
            state.audioElement.src = '';
            state.audioElement.load();
            
            // Remove from DOM if needed
            if (state.audioElement.parentNode) {
                state.audioElement.remove();
            }
            
            state.audioElement = null;
        } catch (e) {
            log(`Error cleaning up audio element: ${e.message}`, 'AUDIO', true);
        }
    }
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
    
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
        state.connectionHealthTimer = null;
    }
    
    // Clean up audio element
    cleanupAudioElement();
    
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
        // Build URL with parameters
        const timestamp = Date.now();
        const position = state.serverPosition || 0;
        
        // Create URL with platform-specific parameters
        let streamUrl = `/direct-stream?t=${timestamp}&position=${position}`;
        
        // Add platform info to help server optimize delivery
        if (state.isIOS) {
            streamUrl += '&platform=ios';
        } else if (state.isSafari) {
            streamUrl += '&platform=safari';
        } else if (state.isMobile) {
            streamUrl += '&platform=mobile';
        }
        
        log(`Using stream URL: ${streamUrl}`, 'PLAYBACK');
        
        // Set the audio source
        state.audioElement.src = streamUrl;
        
        // Special handling for iOS
        if (state.isIOS) {
            // iOS requires user interaction to start playback
            showStatus('Ready - Tap play to start streaming', false, false);
            startBtn.textContent = 'Play';
            startBtn.disabled = false;
            startBtn.dataset.connected = 'true';
            
            // Setup special click handler for iOS
            startBtn.onclick = function() {
                if (!state.audioElement) return;
                
                if (state.audioElement.paused) {
                    startBtn.disabled = true;
                    showStatus('Starting playback...', false, false);
                    
                    // Short delay for iOS
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
                    
                    // Reset onclick to normal toggle behavior
                    startBtn.onclick = toggleConnection;
                } else {
                    stopAudio();
                }
            };
        } else {
            // For other browsers, try to play automatically
            showStatus('Starting playback...', false, false);
            
            // Add a slight delay before playing
            setTimeout(() => {
                if (!state.audioElement || !state.isPlaying) return;
                
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
                        
                        // Setup click handler for user interaction
                        startBtn.onclick = function() {
                            if (!state.audioElement) return;
                            
                            if (state.audioElement.paused && state.isPlaying) {
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
                                
                                // Reset onclick to normal toggle behavior
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
            }, 200);
        }
    } catch (e) {
        log(`Direct streaming error: ${e.message}`, 'AUDIO', true);
        showStatus(`Streaming error: ${e.message}`, true);
        stopAudio(true);
    }
}

// Check connection health
function checkConnectionHealth() {
    if (!state.isPlaying) return;
    
    // Check if we should update now playing
    const now = Date.now();
    const timeSinceLastTrackInfo = (now - state.lastTrackInfoTime) / 1000;
    
    if (timeSinceLastTrackInfo > config.NOW_PLAYING_INTERVAL / 1000) {
        fetchNowPlaying();
    }
    
    // Check if audio element is paused when it shouldn't be
    if (state.audioElement && state.audioElement.paused && state.isPlaying) {
        log('Audio is paused unexpectedly', 'HEALTH', true);
        attemptReconnection();
    }
}

// Attempt reconnection with exponential backoff
function attemptReconnection() {
    // Don't try to reconnect if we're not supposed to be playing
    if (!state.isPlaying) return;
    
    // Check if we've reached the maximum attempts
    if (state.reconnectAttempts >= config.RECONNECT_ATTEMPTS) {
        log(`Maximum reconnection attempts (${config.RECONNECT_ATTEMPTS}) reached`, 'CONTROL', true);
        showStatus('Could not reconnect to server. Please try again later.', true);
        
        // Reset UI
        stopAudio(true);
        return;
    }
    
    // Increment attempts
    state.reconnectAttempts++;
    
    // Calculate delay with exponential backoff
    const delay = Math.min(1000 * Math.pow(1.5, state.reconnectAttempts - 1), 10000);
    
    log(`Reconnection attempt ${state.reconnectAttempts}/${config.RECONNECT_ATTEMPTS} in ${delay/1000}s`, 'CONTROL');
    showStatus(`Reconnecting (${state.reconnectAttempts}/${config.RECONNECT_ATTEMPTS})...`, true, false);
    
    // Clean up existing audio element
    cleanupAudioElement();
    
    // Schedule reconnection
    setTimeout(() => {
        if (state.isPlaying) {
            // Create a new audio element
            state.audioElement = new Audio();
            state.audioElement.controls = false;
            state.audioElement.volume = state.volume;
            state.audioElement.muted = state.isMuted;
            state.audioElement.preload = 'auto';
            
            // Set up event listeners
            setupAudioListeners();
            
            // Retrieve fresh track info before reconnecting
            fetchNowPlaying().then(() => {
                // Start playback with current position
                startDirectPlayback();
            });
        }
    }, delay);
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
            
            // If we're playing, consider reconnecting for new track
            if (state.isPlaying && state.audioElement) {
                log("Track changed, reconnecting to get new track", 'TRACK');
                attemptReconnection();
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
        log(`Received track info: ${data.title || 'Unknown'}`, 'API');
        
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
    log(`Status: ${message}`, 'UI', isError);
    
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

// Logging function
function log(message, category = 'INFO', isError = false) {
    // Always log errors, only log other messages in debug mode
    if (isError || config.DEBUG_MODE) {
        const timestamp = new Date().toISOString().substr(11, 8);
        console[isError ? 'error' : 'log'](`[${timestamp}] [${category}] ${message}`);
    }
}

// Initialize the player when the document is ready
document.addEventListener('DOMContentLoaded', initPlayer);