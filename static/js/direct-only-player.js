// Complete fixed direct-only-player.js with all necessary variables defined

// Player state and configuration
const state = {
    // Audio element 
    audioElement: null,
    
    // Connection and status
    isPlaying: false,
    isMuted: false,
    volume: 0.7,
    reconnectAttempts: 0,
    maxReconnectAttempts: 10,
    lastTrackInfoTime: Date.now(),
    
    // Track info
    currentTrackId: null,
    lastKnownPosition: 0,
    
    // Timers
    nowPlayingTimer: null,
    connectionHealthTimer: null,
    lastErrorTime: 0,
    
    // Buffer monitoring
    lastPlaybackTime: 0,
    poorBufferStartTime: null,
    stalledStartTime: null
};

// Configuration constants 
const config = {
    // Connection settings
    NOW_PLAYING_INTERVAL: 10000,    // Check now playing every 10 seconds
    CONNECTION_CHECK_INTERVAL: 5000, // Check connection health every 5 seconds
    
    // Debug settings
    SHOW_DEBUG_INFO: true,         // Show debug info in console
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

// Detect platform - DEFINED HERE TO FIX THE REFERENCE ERROR
const isAppleDevice = /iPad|iPhone|iPod|Mac/.test(navigator.userAgent) && !window.MSStream;
const isSafari = /^((?!chrome|android).)*safari/i.test(navigator.userAgent);
const isMobile = /Mobi|Android/i.test(navigator.userAgent);

// Initialize player
function initPlayer() {
    log(`Initializing direct streaming radio player... Platform: ${isMobile ? 'Mobile' : 'Desktop'} (Apple: ${isAppleDevice}, Safari: ${isSafari})`);
    
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
        state.volume = this.value;
        
        if (state.audioElement) {
            state.audioElement.volume = state.volume;
        }
        
        try {
            localStorage.setItem('radioVolume', state.volume);
        } catch (e) {
            // Ignore storage errors
        }
    });
    
    // Load saved volume from localStorage
    try {
        const savedVolume = localStorage.getItem('radioVolume');
        if (savedVolume !== null) {
            volumeControl.value = savedVolume;
            state.volume = parseFloat(savedVolume);
        }
    } catch (e) {
        // Ignore storage errors
    }
    
    // Check network connection type if available
    if (navigator.connection) {
        state.connectionType = navigator.connection.effectiveType;
        log(`Network connection type: ${state.connectionType}`);
    }
    
    // Fetch initial track info
    fetchNowPlaying();
    
    log('ChillOut Radio player initialized');
}

// Start audio playback
function startAudio() {
    log('Starting direct audio streaming', 'CONTROL');
    startBtn.disabled = true;
    showStatus('Connecting to stream...', false, false);
    
    // Reset state
    state.reconnectAttempts = 0;
    state.lastTrackInfoTime = Date.now();
    state.isPlaying = true;
    
    // Create audio element if needed
    if (!state.audioElement) {
        state.audioElement = new Audio();
        state.audioElement.controls = false;
        state.audioElement.volume = state.volume;
        state.audioElement.muted = state.isMuted;
        state.audioElement.preload = 'auto';
        
        // iOS/Safari specific settings
        if (isAppleDevice || isSafari) {
            // These properties can help with iOS playback
            state.audioElement.preload = 'auto';
            state.audioElement.autoplay = false;
        }
        
        // Add to document but hide visually
        state.audioElement.style.display = 'none';
        document.body.appendChild(state.audioElement);
        
        // Set up audio event listeners
        setupAudioListeners();
    }
    
    // Start direct streaming
    startDirectPlayback();
    
    // Set up now playing update timer
    if (state.nowPlayingTimer) {
        clearInterval(state.nowPlayingTimer);
    }
    state.nowPlayingTimer = setInterval(fetchNowPlaying, config.NOW_PLAYING_INTERVAL);
    
    // Start connection health check timer
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
    }
    state.connectionHealthTimer = setInterval(checkConnectionHealth, config.CONNECTION_CHECK_INTERVAL);
}

// Stop audio playback
function stopAudio(isError = false) {
    log(`Stopping audio playback${isError ? ' (due to error)' : ''}`, 'CONTROL');
    
    state.isPlaying = false;
    
    // Clear all timers
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
        state.connectionHealthTimer = null;
    }
    
    if (state.nowPlayingTimer) {
        clearInterval(state.nowPlayingTimer);
        state.nowPlayingTimer = null;
    }
    
    // Stop audio properly
    if (state.audioElement) {
        const element = state.audioElement;
        
        // First set src to empty to stop any activity
        try {
            element.src = '';
            element.load();
        } catch (e) {
            // Ignore errors
        }
        
        // Then pause (should be safe now that src is empty)
        try {
            element.pause();
        } catch (e) {
            // Ignore errors
        }
        
        // Remove element from DOM to fully clean up
        try {
            element.remove();
        } catch (e) {
            // Ignore errors
        }
        
        // Clear reference
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

// Set up audio event listeners
function setupAudioListeners() {
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
        
        // If we're stalled for too long, try reconnecting
        const now = Date.now();
        if (now - state.lastErrorTime > 10000) { // Don't reconnect more than once every 10 seconds
            state.lastErrorTime = now;
            setTimeout(() => {
                // Only reconnect if still stalled
                if (state.audioElement && state.audioElement.readyState < 3 && state.isPlaying) {
                    log('Stream still stalled - attempting reconnection', 'AUDIO');
                    attemptReconnection();
                }
            }, 5000); // Wait 5 seconds before attempting reconnection
        }
    });
    
    state.audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        log(`Audio error (code ${errorCode})`, 'AUDIO', true);
        
        // Only react to errors if we're still trying to play
        if (state.isPlaying) {
            // Don't react to errors too frequently
            const now = Date.now();
            if (now - state.lastErrorTime > 10000) { // At most one error response per 10 seconds
                state.lastErrorTime = now;
                showStatus('Audio error - attempting to recover', true, false);
                
                // Try reconnecting
                attemptReconnection();
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
    
    // Extra listeners for better experience
    state.audioElement.addEventListener('canplay', () => {
        log('Audio can play', 'AUDIO');
        showStatus('Stream ready', false, true);
    });
    
    state.audioElement.addEventListener('canplaythrough', () => {
        log('Audio can play through', 'AUDIO');
        // Clear any buffering messages
        if (statusMessage.textContent.includes('Buffering') || 
            statusMessage.textContent.includes('stalled')) {
            showStatus('Playback resumed', false, true);
        }
    });
    
    // Progress monitoring for stats
    state.audioElement.addEventListener('progress', () => {
        const bufferInfo = getBufferInfo();
        if (config.SHOW_DEBUG_INFO && Math.random() < 0.2) {
            log(`Buffered ${bufferInfo.totalSeconds.toFixed(1)} seconds of audio in ${bufferInfo.ranges} ranges`, 'BUFFER');
        }
    });
}

// Start direct HTTP streaming
function startDirectPlayback() {
    try {
        // Set up audio element for direct streaming
        const timestamp = Date.now(); // Prevent caching
        const platformParam = isAppleDevice ? '&platform=ios' : '';
        state.audioElement.src = `/direct-stream?t=${timestamp}${platformParam}`;
        
        // Platform specific handling
        if (isAppleDevice || isSafari) {
            // iOS/Safari may need a user interaction to start playback
            showStatus('Ready - Click play to start streaming', false, false);
            startBtn.textContent = 'Play';
            startBtn.disabled = false;
            startBtn.dataset.connected = 'true';
            
            // Setup click handler for playback
            startBtn.onclick = function() {
                if (state.audioElement.paused) {
                    startBtn.disabled = true;
                    state.audioElement.play().then(() => {
                        showStatus('Stream playing');
                        startBtn.textContent = 'Disconnect';
                        startBtn.disabled = false;
                    }).catch(e => {
                        log(`iOS play failed: ${e.message}`, 'AUDIO', true);
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
            // For other browsers, play automatically
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
                    showStatus('Click play to start audio (browser requires user interaction)', true, false);
                    startBtn.disabled = false;
                    startBtn.dataset.connected = 'true';
                    
                    // Setup click handler for playback
                    startBtn.onclick = function() {
                        if (state.audioElement.paused) {
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
        }
    } catch (e) {
        log(`Direct streaming error: ${e.message}`, 'AUDIO', true);
        showStatus(`Streaming error: ${e.message}`, true);
        stopAudio(true);
    }
}

// Get detailed buffer info
function getDetailedBufferInfo() {
    if (!state.audioElement || !state.audioElement.buffered) {
        return {
            currentTime: 0,
            bufferedRanges: [],
            readyState: 0,
            networkState: 0
        };
    }
    
    const buffered = state.audioElement.buffered;
    const ranges = [];
    
    for (let i = 0; i < buffered.length; i++) {
        ranges.push({
            start: buffered.start(i),
            end: buffered.end(i),
            duration: buffered.end(i) - buffered.start(i)
        });
    }
    
    return {
        currentTime: state.audioElement.currentTime,
        bufferedRanges: ranges,
        readyState: state.audioElement.readyState,
        networkState: state.audioElement.networkState,
        paused: state.audioElement.paused,
        ended: state.audioElement.ended,
        seeking: state.audioElement.seeking
    };
}

// Check connection health
function checkConnectionHealth() {
    if (!state.isPlaying) return;
    
    const timeSinceLastTrackInfo = (Date.now() - state.lastTrackInfoTime) / 1000;
    
    // Check if we need to update now playing info
    if (timeSinceLastTrackInfo > config.NOW_PLAYING_INTERVAL / 1000) {
        fetchNowPlaying();
    }
    
    // For direct streaming, check if it's still playing or paused unexpectedly
    if (state.audioElement) {
        // Get detailed buffer info for better diagnosis
        const bufferInfo = getDetailedBufferInfo();
        
        // Log buffer status periodically for monitoring
        if (config.SHOW_DEBUG_INFO || state.audioElement.readyState < 3) {
            log(`Buffer: readyState=${bufferInfo.readyState}, networkState=${bufferInfo.networkState}, ranges=${bufferInfo.bufferedRanges.length}, paused=${bufferInfo.paused}`, 'HEALTH');
        }
        
        // Check for problems - paused unexpectedly
        if (state.audioElement.paused && state.isPlaying) {
            // Only if not a very recent error
            const now = Date.now();
            if (now - state.lastErrorTime > 10000) {
                log('Stream paused unexpectedly', 'HEALTH', true);
                showStatus('Stream interrupted. Reconnecting...', true, false);
                attemptReconnection();
                return;
            }
        }
        
        // Check if we have zero buffer and poor network state
        if (state.audioElement.readyState < 2 && bufferInfo.bufferedRanges.length === 0) {
            log(`Poor buffer state: readyState=${state.audioElement.readyState}, no buffered data`, 'HEALTH', true);
            
            // If we've been in a poor buffer state for a while, reconnect
            if (!state.poorBufferStartTime) {
                state.poorBufferStartTime = Date.now();
            } else if (Date.now() - state.poorBufferStartTime > 5000) { // 5 seconds in poor buffer state
                log('Persistent poor buffer - attempting reconnection', 'HEALTH', true);
                state.poorBufferStartTime = null; // Reset timer
                attemptReconnection();
                return;
            }
        } else {
            // Reset poor buffer timer if we're in a good state
            state.poorBufferStartTime = null;
        }
        
        // Check for stalled playback
        if (state.audioElement.readyState >= 3 && !state.audioElement.paused && 
            state.lastPlaybackTime !== undefined && 
            state.audioElement.currentTime === state.lastPlaybackTime) {
            
            if (!state.stalledStartTime) {
                state.stalledStartTime = Date.now();
            } else if (Date.now() - state.stalledStartTime > 3000) { // 3 seconds stalled
                log('Playback stalled despite having buffer - attempting reconnection', 'HEALTH', true);
                state.stalledStartTime = null;
                attemptReconnection();
                return;
            }
        } else {
            state.stalledStartTime = null;
        }
        
        // Update last playback time
        state.lastPlaybackTime = state.audioElement.currentTime;
    }
}

// Get buffer information from audio element
function getBufferInfo() {
    if (!state.audioElement || !state.audioElement.buffered || state.audioElement.buffered.length === 0) {
        return { ranges: 0, totalSeconds: 0 };
    }
    
    let totalSeconds = 0;
    const buffered = state.audioElement.buffered;
    
    for (let i = 0; i < buffered.length; i++) {
        totalSeconds += buffered.end(i) - buffered.start(i);
    }
    
    return {
        ranges: buffered.length,
        totalSeconds
    };
}

// Attempt reconnection with exponential backoff
function attemptReconnection() {
    // Don't try to reconnect if we're not supposed to be playing
    if (!state.isPlaying) return;
    
    // Check if we've reached the maximum attempts
    if (state.reconnectAttempts >= state.maxReconnectAttempts) {
        log(`Maximum reconnection attempts (${state.maxReconnectAttempts}) reached`, 'CONTROL', true);
        showStatus('Could not reconnect to server. Please try again later.', true);
        
        // Reset UI
        stopAudio(true);
        return;
    }
    
    // Increment attempts
    state.reconnectAttempts++;
    
    // Calculate delay with exponential backoff and a bit of randomness
    const baseDelay = Math.min(500 * Math.pow(1.3, state.reconnectAttempts - 1), 5000);
    const jitter = Math.random() * 1000; // Add up to 1 second of jitter
    const delay = baseDelay + jitter;
    
    log(`Reconnection attempt ${state.reconnectAttempts}/${state.maxReconnectAttempts} in ${(delay/1000).toFixed(1)}s`, 'CONTROL');
    showStatus(`Reconnecting (${state.reconnectAttempts}/${state.maxReconnectAttempts})...`, true, false);
    
    // IMPORTANT: Always detach the existing audio element and create a new one
    // This prevents the "play() request was interrupted by a call to pause()" error
    let oldElement = state.audioElement;
    
    // Create a new audio element
    state.audioElement = new Audio();
    state.audioElement.controls = false;
    state.audioElement.volume = state.volume;
    state.audioElement.muted = state.isMuted;
    state.audioElement.preload = 'auto';
    state.audioElement.style.display = 'none';
    document.body.appendChild(state.audioElement);
    
    // Set up audio listeners on the new element
    setupAudioListeners();
    
    // Now it's safe to dispose of the old element
    if (oldElement) {
        try {
            oldElement.pause();
            oldElement.src = '';
            oldElement.load();
            oldElement.remove();
        } catch (e) {
            // Ignore errors during cleanup
        }
    }
    
    // Schedule reconnection
    setTimeout(() => {
        if (state.isPlaying) {
            log(`Starting reconnection attempt ${state.reconnectAttempts}`, 'CONTROL');
            
            // Try playback with fresh source and new audio element
            const timestamp = Date.now(); // Prevent caching
            const platformParam = isAppleDevice ? '&platform=ios' : '';
            state.audioElement.src = `/direct-stream?t=${timestamp}${platformParam}`;
            
            // Add a small delay before playing to ensure the browser is ready
            setTimeout(() => {
                if (state.isPlaying) {
                    state.audioElement.play().then(() => {
                        log('Reconnection successful', 'CONTROL');
                        showStatus('Reconnected to stream');
                        startBtn.textContent = 'Disconnect';
                        startBtn.disabled = false;
                        startBtn.dataset.connected = 'true';
                        
                        // Reset reconnect attempts on successful connection
                        state.reconnectAttempts = 0;
                    }).catch(e => {
                        log(`Reconnection playback error: ${e.message}`, 'AUDIO', true);
                        
                        // Try again with next attempt
                        if (state.reconnectAttempts < state.maxReconnectAttempts) {
                            log('Will try again on next attempt', 'CONTROL');
                            // Next attempt will happen via health check
                        } else {
                            showStatus('Failed to reconnect after multiple attempts. Please try again later.', true);
                            stopAudio(true);
                        }
                    });
                }
            }, 100);
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
        
        // Store track ID for change detection
        const newTrackId = info.path;
        if (state.currentTrackId !== newTrackId) {
            log(`Track changed to: ${info.title}`, 'TRACK');
            state.currentTrackId = newTrackId;
            
            // Reset position tracking
            state.lastKnownPosition = 0;
        }
        
        // Update UI
        currentTitle.textContent = info.title || 'Unknown Title';
        currentArtist.textContent = info.artist || 'Unknown Artist';
        currentAlbum.textContent = info.album || 'Unknown Album';
        
        // Update progress
        if (info.duration) {
            currentDuration.textContent = formatTime(info.duration);
        }
        
        if (info.playback_position !== undefined) {
            state.lastKnownPosition = info.playback_position;
            updateProgressBar(info.playback_position, info.duration);
        }
        
        // Update listener count
        if (info.active_listeners !== undefined) {
            listenerCount.textContent = `Listeners: ${info.active_listeners}`;
        }
        
        // Store track ID in DOM for future comparison
        currentTitle.dataset.trackId = info.path;
        
        // Update page title
        document.title = `${info.title} - ${info.artist} | ChillOut Radio`;
        
        // Update last track info time
        state.lastTrackInfoTime = Date.now();
    } catch (e) {
        log(`Error processing track info: ${e.message}`, 'TRACK', true);
    }
}

// Fetch now playing info via API
async function fetchNowPlaying() {
    try {
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            log(`Now playing API error: ${response.status}`, 'API', true);
            return;
        }
        
        const data = await response.json();
        updateTrackInfo(data);
    } catch (error) {
        log(`Error fetching now playing: ${error.message}`, 'API', true);
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
            statusMessage.style.display = 'none';
        }, 3000);
    }
}

// Logging function
function log(message, category = 'INFO', isError = false) {
    const timestamp = new Date().toISOString().substr(11, 8);
    console[isError ? 'error' : 'log'](`[${timestamp}] [${category}] ${message}`);
}

// Initialize the player on document ready
document.addEventListener('DOMContentLoaded', initPlayer);