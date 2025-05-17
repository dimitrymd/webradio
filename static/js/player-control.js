// Updated player-control.js for direct streaming on all platforms

// Direct streaming implementation for all platforms
function startAudio() {
    log('Starting audio playback via direct streaming', 'CONTROL');
    startBtn.disabled = true;
    showStatus('Connecting to stream...', false, false);
    
    // Reset state
    state.reconnectAttempts = 0;
    state.bufferUnderflows = 0;
    
    // Create audio element if needed
    if (!state.audioElement) {
        state.audioElement = new Audio();
        state.audioElement.controls = false;
        state.audioElement.volume = volumeControl ? volumeControl.value : 0.7;
        state.audioElement.muted = state.isMuted;
        
        // Add attributes for better mobile compatibility
        state.audioElement.setAttribute('playsinline', '');
        state.audioElement.setAttribute('webkit-playsinline', '');
        
        // Add to document but hide visually
        state.audioElement.style.display = 'none';
        document.body.appendChild(state.audioElement);
        
        // Set up basic audio listeners
        setupAudioListeners();
    }
    
    // Start direct streaming
    startDirectStream();
}

// Direct HTTP streaming implementation
function startDirectStream() {
    state.isPlaying = true;
    
    // First, fetch current track info to get the server position
    fetchNowPlaying().then(() => {
        // Create a direct stream URL with timestamp to prevent caching
        const timestamp = Date.now();
        const streamUrl = `/direct-stream?t=${timestamp}`;
        
        log(`Connecting to direct stream: ${streamUrl}`, 'CONTROL');
        
        // Set up a load event handler to set the current time after loading
        state.audioElement.onloadedmetadata = function() {
            // Get the server position from the API fetch we just did
            const serverPosition = state.serverPosition || 0;
            
            if (serverPosition > 0) {
                log(`Setting playback position to server position: ${serverPosition}s`, 'AUDIO');
                
                // Set the current time to match server position
                // Subtract a small buffer to ensure smooth playback
                const targetPosition = Math.max(0, serverPosition - 3);
                
                try {
                    state.audioElement.currentTime = targetPosition;
                } catch (e) {
                    log(`Error setting currentTime: ${e.message}`, 'AUDIO', true);
                }
            }
            
            // Remove the handler after using it
            state.audioElement.onloadedmetadata = null;
        };
        
        // Set the source 
        state.audioElement.src = streamUrl;
        
        // Try to play
        const playPromise = state.audioElement.play();
        
        // Handle play promise
        if (playPromise !== undefined) {
            playPromise.then(() => {
                log('Direct stream playback started successfully', 'AUDIO');
                showStatus('Streaming started');
                
                // Update UI
                startBtn.textContent = 'Disconnect';
                startBtn.disabled = false;
                startBtn.dataset.connected = 'true';
                
                // Start polling for track info
                startNowPlayingPolling();
                
                // Start track position monitor
                startTrackPositionMonitor();
                
            }).catch(e => {
                log(`Error starting direct stream: ${e.message}`, 'AUDIO', true);
                
                if (e.name === 'NotAllowedError') {
                    showStatus('Tap play button to start audio (browser requires user interaction)', true, false);
                    setupUserInteractionHandlers();
                } else {
                    showStatus(`Playback error: ${e.message}`, true);
                    stopDirectStream();
                }
                
                startBtn.disabled = false;
            });
        }
    }).catch(() => {
        // If we couldn't fetch the server position, just start from the beginning
        log('Could not fetch server position, starting from beginning', 'AUDIO', true);
        
        const timestamp = Date.now();
        const streamUrl = `/direct-stream?t=${timestamp}`;
        
        // Set the source and play
        state.audioElement.src = streamUrl;
        state.audioElement.play().catch(e => {
            log(`Error starting direct stream: ${e.message}`, 'AUDIO', true);
            stopDirectStream();
            startBtn.disabled = false;
        });
    });
}

// Set up listeners for direct streaming
function setupAudioListeners() {
    // For performance, use passive event listeners where appropriate
    const passiveOpts = { passive: true };
    
    state.audioElement.addEventListener('playing', () => {
        log('Audio playing', 'AUDIO');
        showStatus('Audio playing');
    }, passiveOpts);
    
    state.audioElement.addEventListener('waiting', () => {
        log('Audio buffering', 'AUDIO');
        showStatus('Buffering...', false, false);
        
        // Track buffer starvation
        state.bufferUnderflows++;
    });
    
    state.audioElement.addEventListener('stalled', () => {
        log('Audio stalled', 'AUDIO');
        showStatus('Stream stalled - buffering', true, false);
    });
    
    state.audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        log(`Audio error (code ${errorCode})`, 'AUDIO', true);
        
        // Only attempt recovery if we're trying to play
        if (state.isPlaying) {
            showStatus('Audio error - attempting to recover', true, false);
            restartDirectStream();
        }
    });
    
    state.audioElement.addEventListener('ended', () => {
        log('Audio ended', 'AUDIO');
        
        // If we're still supposed to be playing, try to restart
        if (state.isPlaying) {
            log('Audio ended - checking if track change or error', 'AUDIO');
            
            // Check if this is a normal track end or an error
            const now = Date.now();
            const timeSinceLastChange = (now - state.lastTrackChange) / 1000;
            
            if (timeSinceLastChange > 10 && state.trackPlaybackDuration > 0 && 
                timeSinceLastChange < state.trackPlaybackDuration * 1.1) {
                // Normal track end - wait for next polling cycle to refresh
                log('Track appears to have ended normally, waiting for next track', 'AUDIO');
                showStatus('Track ended, loading next...', false, false);
                
                // Force an immediate track info update
                fetchNowPlaying();
                
                // Small delay then restart stream
                setTimeout(restartDirectStream, 1000);
            } else {
                // Possible error - restart immediately
                log('Audio ended unexpectedly, restarting', 'AUDIO', true);
                showStatus('Audio ended unexpectedly - reconnecting', true, false);
                restartDirectStream();
            }
        }
    });
    
    // Add timeupdate handler to update progress bar
    state.audioElement.addEventListener('timeupdate', () => {
        // Only update occasionally to avoid performance issues
        if (Math.random() < 0.1) { // Update roughly every 10th event (~1 second)
            const currentTime = state.audioElement.currentTime;
            
            if (currentDuration && currentPosition && progressBar) {
                updateProgressBar(currentTime, state.trackPlaybackDuration);
            }
        }
    }, passiveOpts);
}

// Add helpers for autoplay restrictions
function setupUserInteractionHandlers() {
    // Function to try playing audio when user interacts with the page
    const tryPlayAudio = function() {
        if (state.audioElement && state.audioElement.paused && state.isPlaying) {
            log('User interaction detected - trying to play audio', 'AUDIO');
            
            state.audioElement.play()
                .then(() => {
                    log('Audio started after user interaction', 'AUDIO');
                    showStatus('Playback started');
                    
                    // Remove these listeners once successful
                    document.removeEventListener('click', tryPlayAudio);
                    document.removeEventListener('touchstart', tryPlayAudio);
                })
                .catch(e => {
                    log(`Still failed to play: ${e.message}`, 'AUDIO', true);
                });
        }
    };
    
    // Add the listeners
    document.addEventListener('click', tryPlayAudio);
    document.addEventListener('touchstart', tryPlayAudio);
}

// Poll for track info
function startNowPlayingPolling() {
    // Clear any existing interval
    if (state.nowPlayingInterval) {
        clearInterval(state.nowPlayingInterval);
    }
    
    // Initial fetch
    fetchNowPlaying();
    
    // Set up polling
    state.nowPlayingInterval = setInterval(() => {
        if (state.isPlaying) {
            fetchNowPlaying();
        } else {
            clearInterval(state.nowPlayingInterval);
            state.nowPlayingInterval = null;
        }
    }, config.NOW_PLAYING_INTERVAL);
}

// Monitor track position for end-of-track detection
function startTrackPositionMonitor() {
    // Clear any existing interval
    if (state.trackPositionInterval) {
        clearInterval(state.trackPositionInterval);
    }
    
    // Set up interval to check position
    state.trackPositionInterval = setInterval(() => {
        if (state.isPlaying && state.audioElement && !state.audioElement.paused) {
            const currentTime = state.audioElement.currentTime;
            const duration = state.trackPlaybackDuration;
            
            // Check if we're near the end of the track
            if (duration > 0 && currentTime > 0 && currentTime >= duration - 1) {
                log(`Near end of track: ${currentTime.toFixed(1)}/${duration}s`, 'TRACK');
                
                // Force a now playing check to prepare for next track
                fetchNowPlaying();
            }
        } else if (!state.isPlaying) {
            clearInterval(state.trackPositionInterval);
            state.trackPositionInterval = null;
        }
    }, config.TRACK_CHECK_INTERVAL);
}

// Restart the direct stream
function restartDirectStream() {
    if (!state.isPlaying) return;
    
    // Don't retry too many times
    if (state.reconnectAttempts >= config.MAX_RETRIES) {
        log(`Maximum reconnection attempts (${config.MAX_RETRIES}) reached`, 'CONTROL', true);
        showStatus('Could not reconnect to stream. Please try again later.', true);
        stopDirectStream();
        return;
    }
    
    state.reconnectAttempts++;
    log(`Restarting direct stream (attempt ${state.reconnectAttempts}/${config.MAX_RETRIES})`, 'CONTROL');
    
    // Fetch latest track info before restarting
    fetchNowPlaying().then(() => {
        // Create a new timestamp to avoid caching
        const timestamp = Date.now();
        const streamUrl = `/direct-stream?t=${timestamp}`;
        
        // Stop the current playback
        state.audioElement.pause();
        
        // Set up a load event handler to set the current time after loading
        state.audioElement.onloadedmetadata = function() {
            // Get the server position from the last track info
            const serverPosition = state.serverPosition || 0;
            
            if (serverPosition > 0) {
                log(`Setting playback position to server position: ${serverPosition}s`, 'AUDIO');
                
                // Set the current time to match server position
                // Subtract a small buffer to ensure smooth playback
                const targetPosition = Math.max(0, serverPosition - 3);
                
                try {
                    state.audioElement.currentTime = targetPosition;
                } catch (e) {
                    log(`Error setting currentTime: ${e.message}`, 'AUDIO', true);
                }
            }
            
            // Remove the handler after using it
            state.audioElement.onloadedmetadata = null;
        };
        
        // Set new source
        state.audioElement.src = streamUrl;
        
        // Small delay to allow for track transition
        setTimeout(() => {
            // Try to play
            const playPromise = state.audioElement.play();
            if (playPromise !== undefined) {
                playPromise.catch(e => {
                    log(`Error restarting stream: ${e.message}`, 'AUDIO', true);
                    
                    if (e.name === 'NotAllowedError') {
                        showStatus('Tap to restart audio', true, false);
                        setupUserInteractionHandlers();
                    } else {
                        // Try again after a delay
                        setTimeout(restartDirectStream, config.RETRY_DELAY);
                    }
                });
            }
        }, 500);
    });
}

// Stop direct streaming
function stopDirectStream() {
    log('Stopping direct stream', 'CONTROL');
    
    state.isPlaying = false;
    
    // Stop polling for track info
    if (state.nowPlayingInterval) {
        clearInterval(state.nowPlayingInterval);
        state.nowPlayingInterval = null;
    }
    
    // Stop track position monitoring
    if (state.trackPositionInterval) {
        clearInterval(state.trackPositionInterval);
        state.trackPositionInterval = null;
    }
    
    // Stop audio playback
    if (state.audioElement) {
        state.audioElement.pause();
        state.audioElement.src = '';
        state.audioElement.load();
    }
    
    // Reset UI
    startBtn.textContent = 'Connect';
    startBtn.disabled = false;
    startBtn.dataset.connected = 'false';
    
    showStatus('Disconnected from stream');
}

// Toggle connection
function toggleConnection() {
    const isConnected = startBtn.dataset.connected === 'true';
    
    if (isConnected) {
        log('User requested disconnect', 'CONTROL');
        stopDirectStream();
    } else {
        log('User requested connect', 'CONTROL');
        startAudio();
    }
}

// Update progress bar function
function updateProgressBar(position, duration) {
    if (progressBar && duration > 0) {
        const percent = (position / duration) * 100;
        progressBar.style.width = `${percent}%`;
        
        // Update text display
        if (currentPosition) currentPosition.textContent = formatTime(position);
        if (currentDuration) currentDuration.textContent = formatTime(duration);
    }
}

// Make functions available to other modules
window.startAudio = startAudio;
window.stopDirectStream = stopDirectStream;
window.toggleConnection = toggleConnection;
window.setupUserInteractionHandlers = setupUserInteractionHandlers;
window.setupAudioListeners = setupAudioListeners;
window.restartDirectStream = restartDirectStream;
window.startNowPlayingPolling = startNowPlayingPolling;
window.updateProgressBar = updateProgressBar;