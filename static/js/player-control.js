function startAudio() {
    log('Starting audio playback via direct streaming', 'CONTROL');
    startBtn.disabled = true;
    showStatus('Connecting to stream...', false, false);
    
    // Reset state
    state.reconnectAttempts = 0;
    state.bufferUnderflows = 0;
    
    // IMPORTANT: For better performance, always create a fresh Audio element
    if (state.audioElement) {
        // Properly clean up existing element
        try {
            state.audioElement.pause();
            state.audioElement.src = '';
            state.audioElement.load();
            state.audioElement.remove();
        } catch (e) {
            // Ignore cleanup errors
        }
    }
    
    // Create a fresh Audio element
    state.audioElement = new Audio();
    state.audioElement.controls = false;
    state.audioElement.volume = volumeControl ? volumeControl.value : 0.7;
    state.audioElement.muted = state.isMuted;
    
    // Add attributes for better mobile compatibility
    state.audioElement.setAttribute('playsinline', '');
    state.audioElement.setAttribute('webkit-playsinline', '');
    
    // ENHANCED BUFFERING: Critical attributes for better buffering
    state.audioElement.setAttribute('preload', 'auto');
    
    // INCREASE BUFFER SIZE: Set high buffer thresholds
    if (state.audioElement.bufferSize !== undefined) {
        try {
            state.audioElement.bufferSize = 512 * 1024; // 512KB buffer if supported
        } catch (e) {
            // Ignore if not supported
        }
    }
    
    // Add to document but hide visually
    state.audioElement.style.display = 'none';
    document.body.appendChild(state.audioElement);
    
    // Set up basic audio listeners with enhanced buffer monitoring
    setupAudioListeners();
    
    // Start direct streaming with timestamp approach and enhanced buffering
    startDirectStreamWithTimestamp();
}

// player-control.js - Part 2/5 - Main streaming function

// Use a timestamp in the URL to indicate the starting position
function startDirectStreamWithTimestamp() {
    state.isPlaying = true;
    
    // First, fetch current track info to get the server position
    fetchNowPlaying().then((trackInfo) => {
        // Get server position
        const serverPosition = trackInfo.playback_position || 0;
        log(`Server position is ${serverPosition}s`, 'AUDIO');
        
        // Track ID is important for ensuring uniqueness
        const trackId = trackInfo.path || 'unknown';
        
        // IMPORTANT: Store track duration and info
        state.trackDuration = trackInfo.duration || 0;
        state.trackPlaybackDuration = trackInfo.duration || 0;
        state.startPosition = serverPosition;
        state.currentTrackId = trackId;
        
        // BUFFERING IMPROVEMENT: Start from slightly earlier position (3s) and increase buffer
        const timestamp = Date.now();
        const positionSec = Math.floor(Math.max(0, serverPosition - 3)); // Start 3 seconds earlier to prevent gaps
        
        // ENHANCED: Request even larger buffer (60s) to reduce pauses during playback
        const streamUrl = `/direct-stream?t=${timestamp}&position=${positionSec}&track=${encodeURIComponent(trackId)}&buffer=60`;
        
        log(`Connecting to direct stream: ${streamUrl}`, 'CONTROL');
        
        // CRITICAL: Set timeout to prevent never-ending load attempts
        state.loadTimeout = setTimeout(() => {
            if (state.isLoading) {
                log('Audio load timeout - proceeding with playback attempt', 'AUDIO', true);
                state.isLoading = false;
                proceedWithPlayback();
            }
        }, 3000); // 3 second timeout for loading
        
        // VERY IMPORTANT: Set loading flag
        state.isLoading = true;
        
        // Set the source URL
        state.audioElement.src = streamUrl;
        
        // ENHANCED: Use the loadeddata event to detect when enough data is loaded
        state.audioElement.addEventListener('loadeddata', function loadedHandler() {
            // Remove this handler to prevent it firing again
            state.audioElement.removeEventListener('loadeddata', loadedHandler);
            
            log('Audio data loaded, proceeding with playback', 'AUDIO');
            state.isLoading = false;
            
            // Clear load timeout
            if (state.loadTimeout) {
                clearTimeout(state.loadTimeout);
                state.loadTimeout = null;
            }
            
            // Proceed with playback
            proceedWithPlayback();
        }, { once: true });
        
        // IMPORTANT: Start preloading data
        try {
            state.audioElement.load();
        } catch (e) {
            log(`Error preloading audio: ${e.message}`, 'AUDIO', true);
            // Continue despite error
            state.isLoading = false;
            proceedWithPlayback();
        }
        
        // Function to proceed with playback after loading or timeout
        function proceedWithPlayback() {
            // Record start time for position calculation
            state.streamStartTime = Date.now();
            
            // Clear any existing timers before setting up new ones
            clearAllTimers();
            
            // Play the stream
            const playPromise = state.audioElement.play();
            
            if (playPromise !== undefined) {
                playPromise.then(() => {
                    log('Direct stream playback started', 'AUDIO');
                    showStatus('Streaming started');
                    
                    // Update UI
                    startBtn.textContent = 'Disconnect';
                    startBtn.disabled = false;
                    startBtn.dataset.connected = 'true';
                    
                    // Start progress calculation right away
                    updateProgressDisplay();
                    
                    // Start monitoring for buffer issues
                    startBufferMonitoring();
                    
                    // Don't start polling immediately
                    setTimeout(() => {
                        startNowPlayingPolling();
                    }, 3000); // Reduced delay
                    
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
        }
    }).catch(() => {
        // If we couldn't fetch server position, start without position info
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

// player-control.js - Part 3/5 - Event listeners and buffer monitoring

// Set up listeners for direct streaming with enhanced buffer monitoring
function setupAudioListeners() {
    // For performance, use passive event listeners where appropriate
    const passiveOpts = { passive: true };
    
    state.audioElement.addEventListener('playing', () => {
        log(`Audio playing`, 'AUDIO');
        showStatus('Audio playing');
        
        // IMPORTANT: When we first start playing, update the display
        updateProgressDisplay();
    }, passiveOpts);
    
    // ENHANCED: More detailed buffer monitoring
    state.audioElement.addEventListener('waiting', () => {
        log('Audio buffering - waiting for data', 'AUDIO');
        showStatus('Buffering...', false, false);
        
        // Track buffer starvation
        state.bufferUnderflows++;
        
        // If we've had too many buffer underflows in a short time, restart
        if (state.bufferUnderflows > 3 && state.isPlaying) {
            const timeSinceStart = (Date.now() - state.streamStartTime) / 1000;
            if (timeSinceStart < 10) {
                // If we're having immediate buffering issues, try restarting
                log('Multiple buffer underflows at start - restarting', 'AUDIO', true);
                restartDirectStreamWithTimestamp();
                return;
            }
        }
        
        // ENHANCEMENT: Try to recover from buffer underflow
        if (state.audioElement && state.isPlaying) {
            // Lower playback rate slightly during recovery to build buffer
            try {
                state.audioElement.playbackRate = 0.95;
                log('Lowered playback rate to recover from buffering', 'AUDIO');
                
                // Restore normal playback rate after a delay
                setTimeout(() => {
                    if (state.audioElement && state.isPlaying) {
                        state.audioElement.playbackRate = 1.0;
                        log('Restored normal playback rate', 'AUDIO');
                    }
                }, 5000);
            } catch (e) {
                // Ignore playback rate errors
            }
        }
    });
    
    state.audioElement.addEventListener('stalled', () => {
        log('Audio stalled - network issue?', 'AUDIO');
        showStatus('Stream stalled - buffering', true, false);
        
        // If we stall for too long, attempt to restart
        if (state.stallTimeout) {
            clearTimeout(state.stallTimeout);
        }
        
        state.stallTimeout = setTimeout(() => {
            if (state.isPlaying && state.audioElement && 
                (state.audioElement.paused || state.audioElement.readyState < 3)) {
                log('Stall timeout - attempting to restart', 'AUDIO', true);
                restartDirectStreamWithTimestamp();
            }
        }, 5000); // Wait 5 seconds before restarting
    });
    
    // ENHANCED: Add progress event monitoring
    state.audioElement.addEventListener('progress', () => {
        // Clear stall timeout if we're making progress
        if (state.stallTimeout) {
            clearTimeout(state.stallTimeout);
            state.stallTimeout = null;
        }
        
        // Check buffer state occasionally
        if (Math.random() < 0.1) { // ~10% of events
            reportBufferState();
        }
    }, passiveOpts);
    
    state.audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        log(`Audio error (code ${errorCode})`, 'AUDIO', true);
        
        // Only attempt recovery if we're trying to play
        if (state.isPlaying) {
            showStatus('Audio error - attempting to recover', true, false);
            restartDirectStreamWithTimestamp();
        }
    });
    
    state.audioElement.addEventListener('ended', () => {
        log('Audio ended event fired', 'AUDIO');
        
        // If we're still supposed to be playing, check if this is really the end
        if (state.isPlaying) {
            // Get current position estimate and track duration
            const startTime = state.streamStartTime || 0;
            const startPosition = state.startPosition || 0;
            const elapsedSec = (Date.now() - startTime) / 1000;
            const estimatedPosition = startPosition + elapsedSec;
            const duration = state.trackDuration || 0;
            
            log(`Ended event at estimated position: ${estimatedPosition.toFixed(1)}s of ${duration}s`, 'AUDIO');
            
            // IMPORTANT: Only consider it a real end if we're very close to the actual end
            // and the track has been playing for a while
            const timeSinceStart = (Date.now() - state.streamStartTime) / 1000;
            
            if (duration > 0 && estimatedPosition > (duration * 0.90) && timeSinceStart > 30) {
                // This is likely a real track end - wait for next polling cycle
                log('Track appears to have ended normally, waiting for next track', 'AUDIO');
                showStatus('Track ended, loading next...', false, false);
                
                // Force an immediate track info update
                fetchNowPlaying();
                
                // Small delay then restart stream
                setTimeout(restartDirectStreamWithTimestamp, 1000);
            } else {
                // This is likely a false end - probably a buffer issue
                log('False end detected - track did not complete. Restarting stream.', 'AUDIO', true);
                showStatus('Reconnecting...', true, false);
                restartDirectStreamWithTimestamp();
            }
        }
    });
    
}

// NEW: Enhanced buffer monitoring to reduce stuttering
function startBufferMonitoring() {
    // Clear any existing buffer monitor
    if (state.bufferMonitorInterval) {
        clearInterval(state.bufferMonitorInterval);
    }
    
    // Set up an interval to check buffer health
    state.bufferMonitorInterval = setInterval(() => {
        if (!state.isPlaying || !state.audioElement) {
            clearInterval(state.bufferMonitorInterval);
            state.bufferMonitorInterval = null;
            return;
        }
        
        // Check buffer state and take action if needed
        const bufferInfo = getBufferInfo();
        
        // If we have less than 2 seconds buffered ahead, try to recover
        if (bufferInfo.aheadSec < 2 && !state.audioElement.paused) {
            log(`Low buffer (${bufferInfo.aheadSec.toFixed(1)}s ahead) - attempting to recover`, 'BUFFER', true);
            
            // Try to pause briefly to let buffer fill
            try {
                // Lower playback rate during buffer stress
                state.audioElement.playbackRate = 0.95;
                log('Lowered playback rate to build buffer', 'AUDIO');
                
                // Restore normal rate after 3 seconds
                setTimeout(() => {
                    if (state.audioElement && state.isPlaying) {
                        state.audioElement.playbackRate = 1.0;
                    }
                }, 3000);
            } catch (e) {
                // Ignore playback rate errors
            }
        } 
        // If we have a comfortable buffer, ensure normal playback rate
        else if (bufferInfo.aheadSec > 8) {
            // Ensure playback rate is normal
            try {
                if (state.audioElement.playbackRate !== 1.0) {
                    state.audioElement.playbackRate = 1.0;
                }
            } catch (e) {
                // Ignore playback rate errors
            }
        }
        
        // Report buffer state occasionally
        if (Math.random() < 0.2) { // 20% chance each check
            reportBufferState();
        }
    }, 1000); // Check every second
}

// NEW: Helper function to get buffer information
function getBufferInfo() {
    if (!state.audioElement || !state.audioElement.buffered || 
        state.audioElement.buffered.length === 0) {
        return { 
            aheadSec: 0, 
            ranges: 0,
            totalSec: 0
        };
    }
    
    const currentTime = state.audioElement.currentTime;
    const buffered = state.audioElement.buffered;
    let aheadSec = 0;
    
    // Find the buffer range that contains the current time
    for (let i = 0; i < buffered.length; i++) {
        if (currentTime >= buffered.start(i) && currentTime < buffered.end(i)) {
            aheadSec = buffered.end(i) - currentTime;
            break;
        }
    }
    
    // Calculate total buffered time
    let totalSec = 0;
    for (let i = 0; i < buffered.length; i++) {
        totalSec += (buffered.end(i) - buffered.start(i));
    }
    
    return {
        aheadSec,
        ranges: buffered.length,
        totalSec
    };
}

// NEW: Helper to report buffer state to console
function reportBufferState() {
    if (!state.audioElement) return;
    
    const bufferInfo = getBufferInfo();
    
    log(`Buffer state: ${bufferInfo.aheadSec.toFixed(1)}s ahead, ` +
        `${bufferInfo.ranges} ranges, ${bufferInfo.totalSec.toFixed(1)}s total`, 'BUFFER');
        
    // Also report playback state
    if (state.audioElement.paused) {
        log('Playback paused', 'AUDIO');
    } else {
        log(`Playback active, rate=${state.audioElement.playbackRate}`, 'AUDIO');
    }
}

// player-control.js - Part 4/5 - Progress tracking and state management

// CRITICAL FIX: Calculate position based on elapsed time since stream start
function updateProgressDisplay() {
    if (!state.isPlaying) return;
    
    // Clear any existing progress timer
    if (state.progressTimer) {
        clearInterval(state.progressTimer);
        state.progressTimer = null;
    }
    
    // Set up a timer to update the progress bar regularly
    state.progressTimer = setInterval(() => {
        if (!state.isPlaying || !state.audioElement) {
            clearInterval(state.progressTimer);
            state.progressTimer = null;
            return;
        }
        
        // If audio is paused and not in a buffering state, try to resume
        if (state.audioElement.paused && state.isPlaying && 
            state.audioElement.readyState >= 3 && // HAVE_FUTURE_DATA
            Date.now() - state.lastBufferAttempt > 2000) { // Don't try too frequently
            
            log('Audio paused but should be playing - attempting to resume', 'AUDIO');
            state.lastBufferAttempt = Date.now();
            
            state.audioElement.play().catch(e => {
                log(`Error resuming playback: ${e.message}`, 'AUDIO', true);
            });
        }
        
        // Calculate current position based on elapsed time since stream start
        const startTime = state.streamStartTime || 0;
        const startPosition = state.startPosition || 0;
        const elapsedSec = (Date.now() - startTime) / 1000;
        const estimatedPosition = startPosition + elapsedSec;
        
        // Get the track duration from stored state
        const duration = state.trackDuration || 0;
        
        // Update progress bar based on our calculated position
        if (duration > 0) {
            const percent = Math.min((estimatedPosition / duration) * 100, 100);
            if (progressBar) {
                progressBar.style.width = `${percent}%`;
            }
            
            // Update text display
            if (currentPosition) {
                currentPosition.textContent = formatTime(estimatedPosition);
            }
            if (currentDuration && currentDuration.textContent !== formatTime(duration)) {
                currentDuration.textContent = formatTime(duration);
            }
            
            // Check if we're near the end of the track
            if (duration > 0 && estimatedPosition > 0 && estimatedPosition >= (duration * 0.95)) {
                log(`Near end of track: ${estimatedPosition.toFixed(1)}/${duration}s (${Math.round(estimatedPosition/duration*100)}%)`, 'TRACK');
                
                // Fetch track info but do NOT automatically restart yet - just prepare
                fetchNowPlaying().then(trackInfo => {
                    // Check if the track has actually changed on the server
                    if (trackInfo && trackInfo.path && trackInfo.path !== state.currentTrackId) {
                        log('Track has changed on server, preparing to restart for the new track', 'TRACK');
                        setTimeout(restartDirectStreamWithTimestamp, 1000);
                    }
                });
            }
        }
    }, 250); // Update 4 times per second for smoother progress
}

// Clear all timers to prevent memory leaks
function clearAllTimers() {
    if (state.nowPlayingInterval) {
        clearInterval(state.nowPlayingInterval);
        state.nowPlayingInterval = null;
    }
    
    if (state.trackPositionInterval) {
        clearInterval(state.trackPositionInterval);
        state.trackPositionInterval = null;
    }
    
    if (state.progressTimer) {
        clearInterval(state.progressTimer);
        state.progressTimer = null;
    }
    
    if (state.bufferMonitorInterval) {
        clearInterval(state.bufferMonitorInterval);
        state.bufferMonitorInterval = null;
    }
    
    if (state.loadTimeout) {
        clearTimeout(state.loadTimeout);
        state.loadTimeout = null;
    }
    
    if (state.stallTimeout) {
        clearTimeout(state.stallTimeout);
        state.stallTimeout = null;
    }
}

// Poll for track info without position manipulation
function startNowPlayingPolling() {
    // Clear any existing interval
    if (state.nowPlayingInterval) {
        clearInterval(state.nowPlayingInterval);
    }
    
    log('Starting now playing polling without position manipulation', 'CONTROL');
    
    // Set up polling
    state.nowPlayingInterval = setInterval(() => {
        if (!state.isPlaying) {
            clearInterval(state.nowPlayingInterval);
            state.nowPlayingInterval = null;
            return;
        }
        
        // Just get the track info without any position syncing
        fetch('/api/now-playing')
            .then(response => response.json())
            .then(data => {
                // Only update the UI, don't touch the audio position
                
                // Update UI
                if (currentTitle) currentTitle.textContent = data.title || 'Unknown Title';
                if (currentArtist) currentArtist.textContent = data.artist || 'Unknown Artist';
                if (currentAlbum) currentAlbum.textContent = data.album || 'Unknown Album';
                
                // Store track duration
                if (data.duration) {
                    // IMPORTANT: Store duration for our position calculation
                    state.trackDuration = data.duration;
                    state.trackPlaybackDuration = data.duration;
                }
                
                // Get the latest server position for progress calculations
                if (data.playback_position !== undefined) {
                    const serverPosition = data.playback_position;
                    
                    // IMPROVEMENT: Periodically sync our client time calculation with server position
                    const startTime = state.streamStartTime || 0;
                    const elapsedSec = (Date.now() - startTime) / 1000;
                    const oldStartPosition = state.startPosition || 0;
                    const estimatedPosition = oldStartPosition + elapsedSec;
                    
                    // Log server position vs our estimate occasionally
                    log(`Server position: ${serverPosition}s, our estimate: ${estimatedPosition.toFixed(1)}s`, 'POSITION');
                    
                    // If our estimate is significantly off from server position, adjust it
                    const diff = Math.abs(estimatedPosition - serverPosition);
                    if (diff > 5) { // More than 5 seconds off
                        log(`Adjusting position calculation (was ${estimatedPosition.toFixed(1)}s, server says ${serverPosition}s)`, 'PROGRESS');
                        
                        // Reset our start time and position for future calculations
                        state.streamStartTime = Date.now();
                        state.startPosition = serverPosition;
                    }
                }
                
                // Update listener count
                if (data.active_listeners !== undefined && listenerCount) {
                    listenerCount.textContent = `Listeners: ${data.active_listeners}`;
                }
                
                // Only check for track change
                const newTrackId = data.path;
                if (state.currentTrackId && newTrackId && state.currentTrackId !== newTrackId) {
                    log(`Track changed on server: ${data.title}`, 'TRACK');
                    
                    // IMPORTANT: Don't restart for every track change!
                    // Only when explicitly near the end
                    
                    // Get the current playback position estimate
                    const startTime = state.streamStartTime || 0;
                    const startPosition = state.startPosition || 0;
                    const elapsedSec = (Date.now() - startTime) / 1000;
                    const estimatedPosition = startPosition + elapsedSec;
                    const duration = state.trackDuration || 0;
                    
                    // Store the track change info for reference
                    state.currentTrackId = newTrackId;
                    state.lastTrackChange = Date.now();
                    
                    // Only restart if we're very close to the end (>90% through)
                    // This prevents false restarts in the middle of playback
                    if (duration > 0 && estimatedPosition > (duration * 0.90)) {
                        log(`Near end of previous track (${Math.round(estimatedPosition/duration*100)}%), restarting for new track`, 'TRACK');
                        setTimeout(restartDirectStreamWithTimestamp, 1000);
                    } else {
                        log(`Track changed on server but we're not near the end (${Math.round(estimatedPosition/duration*100)}%), continuing playback`, 'TRACK');
                    }
                } else if (!state.currentTrackId && newTrackId) {
                    // First update
                    state.currentTrackId = newTrackId;
                }
            })
            .catch(error => {
                log(`Error fetching now playing: ${error.message}`, 'API', true);
            });
    }, 5000); // Every 5 seconds is enough
}

// Function to proceed with playback after loading or timeout
function proceedWithPlayback() {
    // Play
    state.audioElement.play()
        .then(() => {
            log('Restart successful', 'AUDIO');
            // Reset reconnect attempts on success
            state.reconnectAttempts = 0;
                    
            // Start progress display immediately
            updateProgressDisplay();
                    
            // Start buffer monitoring
             startBufferMonitoring();
        })
        .catch(error => {
            log(`Error restarting playback: ${error.message}`, 'AUDIO', true);
                    
        if (error.name === 'NotAllowedError') {
            showStatus('Tap to restart audio', true, false);
            setupUserInteractionHandlers();
        } else {
             // Try again after a delay
            setTimeout(restartDirectStreamWithTimestamp, config.RETRY_DELAY);
        }
    });
}

// Stop direct streaming
function stopDirectStream() {
    log('Stopping direct stream', 'CONTROL');
    
    state.isPlaying = false;
    
    // Clear all timers
    clearAllTimers();
    
    // Stop audio playback
    if (state.audioElement) {
        state.audioElement.pause();
        state.audioElement.src = '';
        try {
            state.audioElement.load();
        } catch (e) {
            // Ignore load errors
        }
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
                    
                    // Start monitoring and progress display
                    updateProgressDisplay();
                    startBufferMonitoring();
                    
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

// Legacy function to maintain compatibility with other modules
// We don't use this anymore since we use our own progress calculation
function updateProgressBar(position, duration) {
    // This is now a no-op, just here for compatibility
    // Progress is handled by updateProgressDisplay()
}

// Make functions available to other modules
window.startAudio = startAudio;
window.stopDirectStream = stopDirectStream;
window.toggleConnection = toggleConnection;
window.setupUserInteractionHandlers = setupUserInteractionHandlers;
window.setupAudioListeners = setupAudioListeners;
window.restartDirectStreamWithTimestamp = restartDirectStreamWithTimestamp;
window.startNowPlayingPolling = startNowPlayingPolling;
window.updateProgressBar = updateProgressBar; // Keep for compatibility
window.updateProgressDisplay = updateProgressDisplay;
window.startBufferMonitoring = startBufferMonitoring;
window.getBufferInfo = getBufferInfo;
window.reportBufferState = reportBufferState;