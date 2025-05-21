// chrome-player.js - Simplified Chrome-specific player

document.addEventListener('DOMContentLoaded', function() {
    // Get UI elements
    const startBtn = document.getElementById('start-btn');
    const muteBtn = document.getElementById('mute-btn');
    const volumeControl = document.getElementById('volume');
    const statusMessage = document.getElementById('status-message');
    
    const currentTitle = document.getElementById('current-title');
    const currentArtist = document.getElementById('current-artist');
    const currentAlbum = document.getElementById('current-album');
    const currentPosition = document.getElementById('current-position');
    const currentDuration = document.getElementById('current-duration');
    const progressBar = document.getElementById('progress-bar');
    const listenerCount = document.getElementById('listener-count');
    
    // Player state
    const state = {
        audioElement: null,
        isPlaying: false,
        isMuted: false,
        volume: 0.7,
        updateTimer: null,
        nowPlayingTimer: null,
        currentTrack: null
    };
    
    // Initialize volume from localStorage if available
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
        // Ignore localStorage errors
    }
    
    // Set up event handlers
    startBtn.addEventListener('click', function() {
        if (state.isPlaying) {
            stopAudio();
        } else {
            startAudio();
        }
    });
    
    muteBtn.addEventListener('click', function() {
        state.isMuted = !state.isMuted;
        if (state.audioElement) {
            state.audioElement.muted = state.isMuted;
        }
        muteBtn.textContent = state.isMuted ? 'Unmute' : 'Mute';
        
        try {
            localStorage.setItem('radioMuted', state.isMuted.toString());
        } catch (e) {
            // Ignore localStorage errors
        }
    });
    
    volumeControl.addEventListener('input', function() {
        state.volume = this.value;
        if (state.audioElement) {
            state.audioElement.volume = state.volume;
        }
        
        try {
            localStorage.setItem('radioVolume', state.volume);
        } catch (e) {
            // Ignore localStorage errors
        }
    });
    
    // Start audio playback
    function startAudio() {
        console.log('Starting audio playback');
        startBtn.disabled = true;
        showStatus('Connecting to stream...', false, false);
        
        // Clean up any existing audio element
        cleanupAudioElement();
        
        // Create new audio element
        state.audioElement = new Audio();
        state.audioElement.volume = state.volume;
        state.audioElement.muted = state.isMuted;
        state.audioElement.preload = 'auto';
        
        // Set up event listeners
        state.audioElement.addEventListener('playing', function() {
            console.log('Audio playing');
            showStatus('Audio playing');
        });
        
        state.audioElement.addEventListener('waiting', function() {
            console.log('Audio buffering');
            showStatus('Buffering...', false, false);
        });
        
        state.audioElement.addEventListener('stalled', function() {
            console.log('Audio stalled');
            showStatus('Stream stalled - buffering', true, false);
        });
        
        state.audioElement.addEventListener('error', function(e) {
            const errorCode = e.target.error ? e.target.error.code : 'unknown';
            const errorMsg = getErrorMessage(e.target.error);
            console.error(`Audio error: ${errorMsg} (code ${errorCode})`);
            showStatus(`Audio error: ${errorMsg}`, true);
            
            // Auto-retry for specific errors
            if (errorCode !== MediaError.MEDIA_ERR_ABORTED) {
                setTimeout(function() {
                    if (state.isPlaying) {
                        console.log('Auto-reconnecting after error');
                        retryConnection();
                    }
                }, 3000);
            }
        });
        
        state.audioElement.addEventListener('ended', function() {
            console.log('Audio ended');
            if (state.isPlaying) {
                console.log('Stream ended unexpectedly, reconnecting');
                showStatus('Stream ended - reconnecting', true, false);
                retryConnection();
            }
        });
        
        // Start playback with Chrome-specific URL
        const timestamp = Date.now();
        state.audioElement.src = `/chrome-stream?t=${timestamp}`;
        
        // Set up timers
        if (state.nowPlayingTimer) {
            clearInterval(state.nowPlayingTimer);
        }
        state.nowPlayingTimer = setInterval(fetchNowPlaying, 10000);
        
        if (state.updateTimer) {
            clearInterval(state.updateTimer);
        }
        state.updateTimer = setInterval(updateProgress, 1000);
        
        // Try to play
        const playPromise = state.audioElement.play();
        if (playPromise !== undefined) {
            playPromise.then(function() {
                state.isPlaying = true;
                startBtn.textContent = 'Disconnect';
                startBtn.disabled = false;
                startBtn.dataset.connected = 'true';
                showStatus('Connected to stream');
                
                // Get track info immediately
                fetchNowPlaying();
            }).catch(function(error) {
                console.error('Play failed:', error);
                
                if (error.name === 'NotAllowedError') {
                    showStatus('Click play to start audio (browser requires user interaction)', true, false);
                    startBtn.disabled = false;
                } else {
                    showStatus(`Play error: ${error.message}`, true);
                    startBtn.disabled = false;
                    cleanupAudioElement();
                }
            });
        }
    }
    
    // Stop audio playback
    function stopAudio() {
        console.log('Stopping audio playback');
        state.isPlaying = false;
        
        // Clear timers
        if (state.nowPlayingTimer) {
            clearInterval(state.nowPlayingTimer);
            state.nowPlayingTimer = null;
        }
        
        if (state.updateTimer) {
            clearInterval(state.updateTimer);
            state.updateTimer = null;
        }
        
        // Clean up audio element
        cleanupAudioElement();
        
        // Update UI
        startBtn.textContent = 'Connect';
        startBtn.disabled = false;
        startBtn.dataset.connected = 'false';
        showStatus('Disconnected from stream');
    }
    
    // Clean up audio element
    function cleanupAudioElement() {
        if (state.audioElement) {
            try {
                state.audioElement.pause();
                state.audioElement.src = '';
                state.audioElement.load();
                state.audioElement = null;
            } catch (e) {
                console.error('Error cleaning up audio element:', e);
            }
        }
    }
    
    // Retry connection after error
    function retryConnection() {
        console.log('Retrying connection');
        showStatus('Reconnecting...', false, false);
        
        // Clean up old audio element
        cleanupAudioElement();
        
        // Create new audio element with retry flag
        state.audioElement = new Audio();
        state.audioElement.volume = state.volume;
        state.audioElement.muted = state.isMuted;
        
        // Set up basic error handler
        state.audioElement.addEventListener('error', function(e) {
            console.error('Error during retry:', e.target.error);
            showStatus('Reconnection failed', true);
            stopAudio();
        });
        
        // Set up success handler
        state.audioElement.addEventListener('playing', function() {
            console.log('Reconnected successfully');
            showStatus('Reconnected to stream');
            
            // Restore normal functionality
            startBtn.textContent = 'Disconnect';
            startBtn.disabled = false;
            startBtn.dataset.connected = 'true';
        });
        
        // Use retry parameter to let server know this is a retry
        const timestamp = Date.now();
        state.audioElement.src = `/chrome-stream?t=${timestamp}&retry=true`;
        
        // Try to play
        state.audioElement.play().catch(function(error) {
            console.error('Retry failed:', error);
            showStatus('Reconnection failed: ' + error.message, true);
            stopAudio();
        });
    }
    
    // Fetch now playing information
    async function fetchNowPlaying() {
        try {
            const response = await fetch('/api/now-playing');
            if (!response.ok) {
                throw new Error(`HTTP error ${response.status}`);
            }
            
            const data = await response.json();
            
            // Store for progress tracking
            state.currentTrack = data;
            
            // Update UI
            currentTitle.textContent = data.title || 'Unknown Title';
            currentArtist.textContent = data.artist || 'Unknown Artist';
            currentAlbum.textContent = data.album || 'Unknown Album';
            
            if (data.duration) {
                currentDuration.textContent = formatTime(data.duration);
            }
            
            if (data.playback_position) {
                updateProgressBar(data.playback_position, data.duration);
            }
            
            if (data.active_listeners) {
                listenerCount.textContent = `Listeners: ${data.active_listeners}`;
            }
            
            // Update page title
            document.title = `${data.title} - ${data.artist} | ChillOut Radio`;
        } catch (error) {
            console.error('Error fetching now playing:', error);
        }
    }
    
    // Update progress using audio element's current time
    function updateProgress() {
        if (!state.audioElement || !state.isPlaying || !state.currentTrack) return;
        
        updateProgressBar(state.audioElement.currentTime, state.currentTrack.duration);
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
    
    // Format time (seconds to MM:SS)
    function formatTime(seconds) {
        if (!seconds) return '0:00';
        const minutes = Math.floor(seconds / 60);
        const secs = Math.floor(seconds % 60);
        return `${minutes}:${secs.toString().padStart(2, '0')}`;
    }
    
    // Show status message
    function showStatus(message, isError = false, autoHide = true) {
        console.log(`Status: ${message}`);
        
        statusMessage.textContent = message;
        statusMessage.style.display = 'block';
        statusMessage.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
        
        if (!isError && autoHide) {
            setTimeout(function() {
                if (statusMessage.textContent === message) {
                    statusMessage.style.display = 'none';
                }
            }, 3000);
        }
    }
    
    // Get human-readable error message
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
                return 'Unknown error';
        }
    }
    
    // Fetch initial track info
    fetchNowPlaying();
    
    // Show ready message
    showStatus('Ready to play', false, false);
    console.log('Chrome-specific player initialized');
});