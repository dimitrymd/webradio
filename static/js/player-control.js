// player-control.js - Part 1: Core Functions

// Force seek to server position function - key to fixing position issue
function forceSeekToServerPosition() {
    // Don't attempt if not playing
    if (!state.isPlaying || !state.audioElement) return;
    
    // Get current track info to determine server position
    fetchNowPlaying().then(trackInfo => {
        // Get the server position
        const serverPosition = trackInfo.playback_position || 0;
        const trackDuration = trackInfo.duration || 0;
        
        console.log(`[POSITION] Server is at position: ${serverPosition}s of ${trackDuration}s`);
        
        // Update our state
        state.startPosition = serverPosition;
        state.trackDuration = trackDuration;
        
        // Force update progress bar
        updateProgressBar(serverPosition, trackDuration);
        
        // Only attempt to seek if we're not already close to the position
        if (state.audioElement && Math.abs(state.audioElement.currentTime - serverPosition) > 3) {
            try {
                console.log(`[POSITION] Attempting forced seek to ${serverPosition}s`);
                
                // Check if seeking is possible
                if (state.audioElement.readyState >= 2) {
                    // Direct seek
                    state.audioElement.currentTime = serverPosition;
                    console.log(`[POSITION] Direct seek successful`);
                } else {
                    console.log(`[POSITION] Audio not ready for seeking (readyState=${state.audioElement.readyState})`);
                    
                    // Setup a listener for when the audio becomes seekable
                    state.audioElement.addEventListener('canplay', function seekWhenReady() {
                        state.audioElement.removeEventListener('canplay', seekWhenReady);
                        
                        try {
                            state.audioElement.currentTime = serverPosition;
                            console.log(`[POSITION] Delayed seek successful`);
                        } catch (e) {
                            console.error(`[POSITION] Delayed seek failed: ${e.message}`);
                        }
                    }, { once: true });
                }
            } catch (e) {
                console.error(`[POSITION] Seek error: ${e.message}`);
            }
        } else {
            console.log(`[POSITION] Already at or near server position, no seek needed`);
        }
    }).catch(error => {
        console.error(`[POSITION] Error fetching server position: ${error.message}`);
    });
}

// Start audio playback
function startAudio() {
    log('Starting audio playback via direct streaming', 'CONTROL');
    startBtn.disabled = true;
    showStatus('Connecting to stream...', false, false);
    
    // Reset state
    state.reconnectAttempts = 0;
    state.bufferUnderflows = 0;
    state.lastBufferEvent = 0;
    state.bufferMetrics = [];
    state.bufferPauseActive = false;
    state.initialStartTime = Date.now();
    state.trackChangeInProgress = false; // Added for track change tracking
    state.lastPlaybackTime = undefined; // Added for stall detection
    state.lastPlaybackCheck = undefined; // Added for stall detection
    state.consecutiveErrors = 0; // Added for error tracking
    
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
    
    // Add to document but hide visually
    state.audioElement.style.display = 'none';
    document.body.appendChild(state.audioElement);
    
    // Set up enhanced audio listeners
    setupEnhancedAudioListeners();
    
    // Start direct streaming with improved buffering
    startDirectStreamWithImprovedBuffering();
}

// Stop direct streaming with proper cleanup
function stopDirectStream() {
    log('Stopping direct stream', 'CONTROL');
    
    state.isPlaying = false;
    
    // Clear all timers
    clearAllTimers();
    
    // Stop audio playback with proper cleanup
    if (state.audioElement) {
        try {
            state.audioElement.pause();
            state.audioElement.src = '';
            state.audioElement.load();
            
            // Remove from DOM
            state.audioElement.remove();
            state.audioElement = null;
        } catch (e) {
            // Ignore cleanup errors
            log(`Error during audio cleanup: ${e.message}`, 'AUDIO');
        }
    }
    
    // Reset state
    state.currentTrackId = null;
    state.bufferUnderflows = 0;
    state.reconnectAttempts = 0;
    state.bufferMetrics = [];
    state.bufferPauseActive = false;
    state.trackChangeInProgress = false;
    state.lastPlaybackTime = undefined;
    state.lastPlaybackCheck = undefined;
    state.consecutiveErrors = 0;
    
    // Reset UI
    startBtn.textContent = 'Connect';
    startBtn.disabled = false;
    startBtn.dataset.connected = 'false';
    
    showStatus('Disconnected from stream');
}

// Toggle connection with debounce
function toggleConnection() {
    const isConnected = startBtn.dataset.connected === 'true';
    
    // Prevent rapid clicking
    if (state.lastToggle && Date.now() - state.lastToggle < 1000) {
        log('Ignoring rapid toggle', 'CONTROL');
        return;
    }
    
    state.lastToggle = Date.now();
    
    if (isConnected) {
        log('User requested disconnect', 'CONTROL');
        stopDirectStream();
    } else {
        log('User requested connect', 'CONTROL');
        startAudio();
    }
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
    
    if (state.rateRestoreTimeout) {
        clearTimeout(state.rateRestoreTimeout);
        state.rateRestoreTimeout = null;
    }
    
    if (state.heartbeatInterval) {
        clearInterval(state.heartbeatInterval);
        state.heartbeatInterval = null;
    }
}

// player-control.js - Part 2: Direct Streaming Implementation

// Improved direct streaming with better error handling and buffering
function startDirectStreamWithImprovedBuffering() {
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
        
        // IMPORTANT: Update the progress bar immediately to show correct position
        updateProgressBar(serverPosition, state.trackDuration);
        
        // BUFFERING IMPROVEMENT: Use the current server position without modification
        const timestamp = Date.now();
        const positionSec = serverPosition; // Use server position exactly
        
        // ENHANCED: Request larger buffer (90s) to reduce pauses during playback
        const streamUrl = `/direct-stream?t=${timestamp}&position=${positionSec}&track=${encodeURIComponent(trackId)}&buffer=90`;
        
        log(`Connecting to direct stream at position ${positionSec}s: ${streamUrl}`, 'CONTROL');
        
        // CRITICAL: Set timeout to prevent never-ending load attempts
        state.loadTimeout = setTimeout(() => {
            if (state.isLoading) {
                log('Audio load timeout - proceeding with playback attempt', 'AUDIO', true);
                state.isLoading = false;
                proceedWithPlayback();
            }
        }, 5000); // Increased timeout for reliable loading
        
        // Set loading flag
        state.isLoading = true;
        
        // Set the source URL
        state.audioElement.src = streamUrl;
        
        // ENHANCED: Better event handling with fallbacks
        // Try to use loadeddata first (most reliable)
        state.audioElement.addEventListener('loadeddata', function loadedHandler() {
            state.audioElement.removeEventListener('loadeddata', loadedHandler);
            
            log('Audio data loaded, proceeding with playback', 'AUDIO');
            state.isLoading = false;
            
            // Clear load timeout
            if (state.loadTimeout) {
                clearTimeout(state.loadTimeout);
                state.loadTimeout = null;
            }
            
            // CRITICAL: Try to set currentTime to match server position before playing
            try {
                if (state.audioElement.seekable && state.audioElement.seekable.length > 0) {
                    log(`Setting current time to match server position: ${serverPosition}s`, 'AUDIO');
                    state.audioElement.currentTime = Math.max(0, serverPosition);
                }
            } catch (e) {
                log(`Error setting current time: ${e.message}`, 'AUDIO', true);
                // Continue anyway - we'll attempt another seek after playback starts
            }
            
            // Proceed with playback
            proceedWithPlayback();
        }, { once: true });
        
        // Fallback for canplay (if loadeddata doesn't fire)
        state.audioElement.addEventListener('canplay', function canPlayHandler() {
            state.audioElement.removeEventListener('canplay', canPlayHandler);
            
            // Only proceed if still loading (not already handled by loadeddata)
            if (state.isLoading) {
                log('Audio can play, proceeding with playback', 'AUDIO');
                state.isLoading = false;
                
                // Clear load timeout
                if (state.loadTimeout) {
                    clearTimeout(state.loadTimeout);
                    state.loadTimeout = null;
                }
                
                // CRITICAL: Try to set currentTime here too for browsers that didn't trigger loadeddata
                try {
                    if (state.audioElement.seekable && state.audioElement.seekable.length > 0) {
                        log(`Setting current time to match server position (canplay): ${serverPosition}s`, 'AUDIO');
                        state.audioElement.currentTime = Math.max(0, serverPosition);
                    }
                } catch (e) {
                    log(`Error setting current time (canplay): ${e.message}`, 'AUDIO', true);
                    // Continue anyway - we'll attempt another seek after playback starts
                }
                
                // Proceed with playback
                proceedWithPlayback();
            }
        }, { once: true });
        
        // Start preloading data
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
            // If already playing, don't try to play again
            if (state.audioElement.paused === false) {
                log('Audio already playing, skipping play attempt', 'AUDIO');
                finishStartup();
                return;
            }
            
            // Record start time for position calculation
            state.streamStartTime = Date.now();
            
            // Clear any existing timers before setting up new ones
            clearAllTimers();
            
            // Play the stream with retry logic
            playWithRetry(3);
            
            function playWithRetry(retriesLeft) {
                log(`Playing audio, ${retriesLeft} retries left`, 'AUDIO');
                
                const playPromise = state.audioElement.play();
                
                if (playPromise !== undefined) {
                    playPromise.then(() => {
                        log('Direct stream playback started', 'AUDIO');
                        
                        // IMPORTANT: Show the current position in the status message
                        const position = state.audioElement.currentTime;
                        const duration = state.trackDuration;
                        const percentage = Math.round((position / duration) * 100);
                        showStatus(`Streaming started at position ${formatTime(position)} (${percentage}%)`);
                        
                        finishStartup();
                    }).catch(e => {
                        log(`Error starting direct stream: ${e.message}`, 'AUDIO', true);
                        
                        if (e.name === 'NotAllowedError') {
                            showStatus('Tap play button to start audio (browser requires user interaction)', true, false);
                            setupUserInteractionHandlers();
                        } else if (retriesLeft > 0) {
                            // Retry with small delay
                            log(`Retrying playback, ${retriesLeft} attempts left`, 'AUDIO');
                            setTimeout(() => playWithRetry(retriesLeft - 1), 1000);
                        } else {
                            showStatus(`Playback error: ${e.message}`, true);
                            stopDirectStream();
                        }
                        
                        startBtn.disabled = false;
                    });
                } else {
                    // For older browsers that don't return a promise
                    log('Play method did not return a promise, assuming playback started', 'AUDIO');
                    finishStartup();
                }
            }
            
            function finishStartup() {
                // Update UI
                startBtn.textContent = 'Disconnect';
                startBtn.disabled = false;
                startBtn.dataset.connected = 'true';
                
                // IMPORTANT: Start progress display with a very short interval initially
                // This ensures we quickly show the correct position
                updateProgressDisplayImmediate();
                
                // Start enhanced buffer monitoring
                startEnhancedBufferMonitoring();
                
                // Start polling with a small delay
                setTimeout(() => {
                    startNowPlayingPolling();
                }, 2000);
                
                // IMPORTANT NEW ADDITION: Force seek after a short delay 
                // This allows the audio to initialize properly first
                setTimeout(forceSeekToServerPosition, 2000);
                
                // Start heartbeat checks
                startHeartbeatChecks();
            }
        }
    }).catch((error) => {
        // If we couldn't fetch server position, start without position info
        log(`Could not fetch server position: ${error.message}, starting from beginning`, 'AUDIO', true);
        
        const timestamp = Date.now();
        const streamUrl = `/direct-stream?t=${timestamp}&buffer=90`; // Request large buffer
        
        // Set the source and play with error handling
        state.audioElement.src = streamUrl;
        
        // Record start time
        state.streamStartTime = Date.now();
        state.startPosition = 0;
        
        // Play with retry on error
        state.audioElement.play().catch(e => {
            log(`Error starting direct stream: ${e.message}`, 'AUDIO', true);
            
            if (e.name === 'NotAllowedError') {
                showStatus('Tap play button to start audio (browser requires user interaction)', true, false);
                setupUserInteractionHandlers();
            } else {
                setTimeout(() => {
                    log('Retrying playback after error', 'AUDIO');
                    state.audioElement.play().catch(e2 => {
                        log(`Second playback attempt also failed: ${e2.message}`, 'AUDIO', true);
                        stopDirectStream();
                    });
                }, 2000);
            }
            
            startBtn.disabled = false;
        });
        
        // Update UI
        startBtn.textContent = 'Disconnect';
        startBtn.disabled = false;
        startBtn.dataset.connected = 'true';
        
        // Setup monitoring
        updateProgressDisplay();
        startEnhancedBufferMonitoring();
        startNowPlayingPolling();
        
        // Start heartbeat checks
        startHeartbeatChecks();
    });
}

// Add immediate progress display update for better UI responsiveness
function updateProgressDisplayImmediate() {
    if (!state.isPlaying || !state.audioElement) return;
    
    // Get current position and duration
    const position = state.audioElement.currentTime || state.startPosition;
    const duration = state.trackDuration || 0;
    
    // Update progress bar immediately
    updateProgressBar(position, duration);
    
    // Update text display
    if (currentPosition) currentPosition.textContent = formatTime(position);
    if (currentDuration && duration > 0) currentDuration.textContent = formatTime(duration);
    
    // Force another quick update in 100ms to handle any race conditions
    setTimeout(() => {
        if (state.isPlaying && state.audioElement) {
            const newPosition = state.audioElement.currentTime || state.startPosition;
            updateProgressBar(newPosition, duration);
            if (currentPosition) currentPosition.textContent = formatTime(newPosition);
        }
    }, 100);
    // player-control.js - Part 3: Event Listeners for Audio Element (continued)

    // Add timeupdate listener for progress tracking
    state.audioElement.addEventListener('timeupdate', () => {
        // Clear stall timeout if we're updating
        if (state.stallTimeout) {
            clearTimeout(state.stallTimeout);
            state.stallTimeout = null;
        }
        
        // Check for track loop condition - if we've exceeded track duration
        if (state.trackDuration > 0 && 
            state.audioElement.currentTime > 0 && 
            state.audioElement.currentTime > state.trackDuration * 1.1) {
            
            log(`Track exceeded expected duration: ${state.audioElement.currentTime.toFixed(1)}s > ${state.trackDuration}s`, 'AUDIO');
            
            // Check if track has changed on server
            fetchNowPlaying().then(trackInfo => {
                if (trackInfo && trackInfo.path && trackInfo.path !== state.currentTrackId) {
                    log('Track changed on server, restarting stream', 'TRACK');
                    restartDirectStreamWithImprovedBuffering();
                }
            });
        }
        
        // IMPORTANT NEW ADDITION: Check for end of track and potential track change
        if (state.trackDuration > 0 && 
            state.audioElement.currentTime > state.trackDuration * 0.98 && 
            !state.trackChangeInProgress) {
            
            log(`Near end of track: ${state.audioElement.currentTime.toFixed(1)}s / ${state.trackDuration}s`, 'TRACK');
            
            // Flag we're checking for track change
            state.trackChangeInProgress = true;
            
            // Force check for track change
            fetchNowPlaying().then(response => {
                if (response && response.path && response.path !== state.currentTrackId) {
                    log('Track changed confirmed via API, restarting stream', 'TRACK');
                    restartDirectStreamWithImprovedBuffering();
                } else {
                    // If we're really at the end but no track change detected,
                    // force a restart anyway after a short delay
                    if (state.audioElement.currentTime > state.trackDuration * 0.99) {
                        log('At end of track but no track change detected, forcing restart soon', 'TRACK');
                        setTimeout(() => {
                            if (!state.trackChangeInProgress) {
                                log('Forcing restart at track end', 'TRACK');
                                restartDirectStreamWithImprovedBuffering();
                            }
                        }, 3000);
                    }
                }
                
                // Clear track change flag after a timeout
                setTimeout(() => {
                    state.trackChangeInProgress = false;
                }, 5000);
            }).catch(() => {
                // Clear track change flag on error
                state.trackChangeInProgress = false;
            });
        }
        
        // IMPORTANT NEW ADDITION: Check for stalled playback by monitoring position changes
        if (state.lastPlaybackTime === undefined) {
            state.lastPlaybackTime = state.audioElement.currentTime;
            state.lastPlaybackCheck = Date.now();
        } else {
            const now = Date.now();
            // Only check every 5 seconds
            if (now - state.lastPlaybackCheck > 5000) {
                const currentTime = state.audioElement.currentTime;
                const timeDiff = currentTime - state.lastPlaybackTime;
                
                // If playback hasn't advanced in 5 seconds, we might be stalled
                if (timeDiff < 0.5) {
                    log(`Playback may be stalled: advanced only ${timeDiff.toFixed(2)}s in 5 seconds`, 'AUDIO');
                    
                    // Check if we are near the end of track
                    const nearEnd = state.trackDuration > 0 && 
                                   currentTime > 0 && 
                                   currentTime > state.trackDuration * 0.95;
                    
                    if (nearEnd) {
                        log(`Near end of track (${currentTime.toFixed(1)}/${state.trackDuration}s), checking for track change`, 'TRACK');
                        // Check if track has changed on server
                        fetchNowPlaying().catch(() => {});
                    } else {
                        // Not near end, might be a connection issue
                        state.consecutiveErrors++;
                        
                        if (state.consecutiveErrors >= 3) {
                            log(`Playback stalled for too long, force refreshing NOW PLAYING info`, 'AUDIO');
                            // Force fetch now playing which should trigger track change if needed
                            fetchNowPlaying().then(trackInfo => {
                                // Check if track has changed
                                if (trackInfo && trackInfo.path && trackInfo.path !== state.currentTrackId) {
                                    log(`Track changed on server, restarting stream`, 'TRACK');
                                    // Track has changed - reloading is needed
                                    setTimeout(restartDirectStreamWithImprovedBuffering, 500);
                                } else {
                                    // Same track, try seeking to current server position 
                                    forceSeekToServerPosition();
                                }
                            }).catch(() => {});
                            
                            state.consecutiveErrors = 0;
                        }
                    }
                } else {
                    // Playback is advancing normally
                    state.consecutiveErrors = 0;
                }
                
                // Update for next check
                state.lastPlaybackTime = currentTime;
                state.lastPlaybackCheck = now;
            }
        }
    }, passiveOpts);
}

// player-control.js - Part 4: Buffer Monitoring and Progress Tracking

// Improved buffer monitoring for direct streaming
function startEnhancedBufferMonitoring() {
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
        
        // Check buffer state
        const bufferInfo = getBufferInfo();
        
        // Track buffer metrics for trend analysis
        state.bufferMetrics.push(bufferInfo.aheadSec);
        if (state.bufferMetrics.length > 10) {
            state.bufferMetrics.shift();
        }
        
        // Calculate buffer trend (negative means decreasing)
        const bufferTrend = state.bufferMetrics.length > 5 ? 
            state.bufferMetrics[state.bufferMetrics.length-1] - state.bufferMetrics[0] : 0;
        
        // Adjust minimum buffer based on history
        const minBuffer = state.bufferUnderflows > 2 ? 3 : 1.5;
        
        // Take action based on buffer health
        if (bufferInfo.aheadSec < minBuffer && !state.bufferPauseActive) {
            log(`Low buffer: ${bufferInfo.aheadSec.toFixed(1)}s ahead, trend: ${bufferTrend.toFixed(2)}`, 'BUFFER');
            
            // If buffer is critically low and continuing to decrease
            if (bufferInfo.aheadSec < 0.5 && bufferTrend < 0) {
                // Try to pause briefly to rebuild buffer
                if (!state.audioElement.paused) {
                    log('Critical buffer level - pausing briefly to rebuild', 'BUFFER', true);
                    state.bufferPauseActive = true;
                    
                    try {
                        state.audioElement.pause();
                        
                        // Resume after short delay
                        setTimeout(() => {
                            if (state.isPlaying && state.audioElement) {
                                log('Resuming after buffer pause', 'BUFFER');
                                state.audioElement.play().catch(e => {
                                    log(`Error resuming: ${e.message}`, 'AUDIO', true);
                                });
                                state.bufferPauseActive = false;
                            }
                        }, 2000);
                    } catch (e) {
                        log(`Error during buffer pause: ${e.message}`, 'AUDIO', true);
                        state.bufferPauseActive = false;
                    }
                }
            } else if (bufferInfo.aheadSec < minBuffer) {
                // Just reduce playback rate to build buffer
                try {
                    // Adjust playback rate based on buffer level
                    const newRate = Math.max(0.9, 1.0 - ((minBuffer - bufferInfo.aheadSec) * 0.1));
                    
                    if (state.audioElement.playbackRate !== newRate) {
                        state.audioElement.playbackRate = newRate;
                        log(`Reduced playback rate to ${newRate.toFixed(2)} to build buffer`, 'BUFFER');
                    }
                } catch (e) {
                    // Ignore playback rate errors
                }
            }
        } else if (bufferInfo.aheadSec > minBuffer * 2 && !state.bufferPauseActive) {
            // Buffer is healthy, ensure normal playback rate
            try {
                if (state.audioElement.playbackRate !== 1.0) {
                    state.audioElement.playbackRate = 1.0;
                    log('Restored normal playback rate', 'BUFFER');
                }
            } catch (e) {
                // Ignore playback rate errors
            }
        }
        
        // Report buffer state less frequently to reduce console spam
        if (state.bufferReportCounter === undefined) {
            state.bufferReportCounter = 0;
        }
        
        state.bufferReportCounter++;
        if (state.bufferReportCounter % 10 === 0) { // Every 10 seconds
            reportBufferState();
        }
    }, 1000); // Check every second
}

// Helper function to get buffer information
function getBufferInfo() {
    if (!state.audioElement || !state.audioElement.buffered || 
        state.audioElement.buffered.length === 0) {
        return { 
            aheadSec: 0, 
            ranges: 0,
            totalSec: 0,
            currentTime: state.audioElement ? state.audioElement.currentTime : 0
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
        totalSec,
        currentTime
    };
}

// Enhanced buffer state reporting
function reportBufferState() {
    if (!state.audioElement) return;
    
    const bufferInfo = getBufferInfo();
    
    // Calculate buffer utilization percentage
    const duration = state.trackDuration || 0;
    const utilization = duration > 0 ? (bufferInfo.totalSec / duration) * 100 : 0;
    
    log(`Buffer: ${bufferInfo.aheadSec.toFixed(1)}s ahead, ` +
        `${bufferInfo.ranges} ranges, ${bufferInfo.totalSec.toFixed(1)}s total (${utilization.toFixed(1)}%)`, 'BUFFER');
        
    // Report playback state
    if (state.audioElement.paused) {
        log('Playback paused', 'AUDIO');
    } else {
        log(`Playback active at ${bufferInfo.currentTime.toFixed(1)}s, rate=${state.audioElement.playbackRate}`, 'AUDIO');
    }
}

// Improved progress tracking
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
        
        // Hybrid position tracking - use audio element time when available
        let position;
        if (!state.audioElement.paused && state.audioElement.readyState >= 3) {
            position = state.audioElement.currentTime;
            
            // Periodically sync our time calculation with actual playback
            if (!state.lastDriftCheck || Date.now() - state.lastDriftCheck > 30000) {
                // Calculate estimated position
                const startTime = state.streamStartTime || 0;
                const startPosition = state.startPosition || 0;
                const elapsedSec = (Date.now() - startTime) / 1000;
                const estimatedPosition = startPosition + elapsedSec;
                
                // Check for significant drift
                const drift = Math.abs(estimatedPosition - position);
                if (drift > 5) {
                    log(`Position drift detected: ${drift.toFixed(1)}s (estimated: ${estimatedPosition.toFixed(1)}s, actual: ${position.toFixed(1)}s)`, 'POSITION');
                    // Adjust our reference point for future calculations
                    state.streamStartTime = Date.now();
                    state.startPosition = position;
                }
                
                state.lastDriftCheck = Date.now();
            }
        } else {
            // Fall back to calculation when audio element isn't reliable
            const startTime = state.streamStartTime || 0;
            const startPosition = state.startPosition || 0;
            const elapsedSec = (Date.now() - startTime) / 1000;
            position = startPosition + elapsedSec;
        }
        
        // Get track duration from stored state
        const duration = state.trackDuration || 0;
        
        // Update progress bar
        if (duration > 0) {
            const percent = Math.min((position / duration) * 100, 100);
            if (progressBar) {
                progressBar.style.width = `${percent}%`;
            }
            
            // Update text display
            if (currentPosition) {
                currentPosition.textContent = formatTime(position);
            }
            if (currentDuration && currentDuration.textContent !== formatTime(duration)) {
                currentDuration.textContent = formatTime(duration);
            }
            
            // Check if we're near the end of the track and need to prepare for transition
            if (duration > 0 && position > 0 && position >= (duration * 0.92)) {
                // Only prepare for transition every 3 seconds to avoid spamming
                const now = Date.now();
                if (!state.lastTransitionCheck || now - state.lastTransitionCheck > 3000) {
                    state.lastTransitionCheck = now;
                    log(`Near end of track: ${position.toFixed(1)}/${duration}s (${Math.round(position/duration*100)}%)`, 'TRACK');
                    
                    // Fetch track info in preparation for potential track change
                    fetchNowPlaying().then(trackInfo => {
                        // Check if track has changed on server
                        if (trackInfo && trackInfo.path && trackInfo.path !== state.currentTrackId) {
                            log('Track has changed on server, preparing to restart for the new track', 'TRACK');
                            // Only restart if we're very close to the end (>95%)
                            if (position >= (duration * 0.95)) {
                                log('At end of track, restarting stream for new track', 'TRACK');
                                setTimeout(restartDirectStreamWithImprovedBuffering, 500);
                            }
                        }
                    }).catch(() => {
                        // Ignore fetch errors to prevent unnecessary reconnections
                    });
                }
            }
        }
    }, 250); // Update 4 times per second for smoother progress
}

// player-control.js - Part 5: Track Info Polling and Heartbeat

// Start server heartbeat checks to detect issues
function startHeartbeatChecks() {
    // Clear any existing interval
    if (state.heartbeatInterval) {
        clearInterval(state.heartbeatInterval);
        state.heartbeatInterval = null;
    }
    
    // Initialize error tracking
    if (state.serverErrorCount === undefined) {
        state.serverErrorCount = 0;
    }
    
    // Set up a regular heartbeat with the server
    state.heartbeatInterval = setInterval(() => {
        if (state.isPlaying) {
            // Simple ping to the server to check health
            fetch('/api/now-playing')
                .then(response => {
                    if (!response.ok) {
                        log(`Server returned status ${response.status}`, 'HEARTBEAT');
                        if (response.status >= 500) {
                            // Server error, may need restart
                            if (!state.serverErrorDetected) {
                                state.serverErrorDetected = true;
                                log(`Server error detected`, 'HEARTBEAT');
                                
                                // After multiple errors, try reconnecting
                                if (state.serverErrorCount++ > 2) {
                                    log(`Multiple server errors, attempting reconnection`, 'HEARTBEAT');
                                    restartDirectStreamWithImprovedBuffering();
                                    state.serverErrorCount = 0;
                                }
                            }
                        }
                    } else {
                        // Server is responding normally
                        state.serverErrorDetected = false;
                        state.serverErrorCount = 0;
                    }
                })
                .catch(error => {
                    log(`Heartbeat error: ${error.message}`, 'HEARTBEAT');
                    state.serverErrorCount++;
                    
                    // After multiple connection errors, try reconnecting
                    if (state.serverErrorCount > 3) {
                        log(`Multiple heartbeat failures, attempting reconnection`, 'HEARTBEAT');
                        restartDirectStreamWithImprovedBuffering();
                        state.serverErrorCount = 0;
                    }
                });
        }
    }, 30000); // Check every 30 seconds
}

// Improved now playing polling with better track change detection
function startNowPlayingPolling() {
    // Clear any existing interval
    if (state.nowPlayingInterval) {
        clearInterval(state.nowPlayingInterval);
    }
    
    log('Starting now playing polling with improved track change detection', 'CONTROL');
    
    // Set up polling with progressive interval
    let pollingInterval = 5000; // Start with 5 seconds
    
    state.nowPlayingInterval = setInterval(() => {
        if (!state.isPlaying) {
            clearInterval(state.nowPlayingInterval);
            state.nowPlayingInterval = null;
            return;
        }
        
        // Get current position and duration
        const position = state.audioElement ? state.audioElement.currentTime : 0;
        const duration = state.trackDuration || 0;
        
        // Adjust polling frequency based on position in track
        if (duration > 0) {
            const percentage = position / duration;
            
            // Poll more frequently when nearing the end of a track
            if (percentage > 0.9) {
                pollingInterval = 2000; // Every 2 seconds near the end
            } else if (percentage > 0.7) {
                pollingInterval = 4000; // Every 4 seconds in last third
            } else {
                pollingInterval = 8000; // Every 8 seconds otherwise
            }
            
            // Update interval if needed
            if (state.nowPlayingInterval) {
                clearInterval(state.nowPlayingInterval);
                state.nowPlayingInterval = setInterval(arguments.callee, pollingInterval);
            }
        }
        
        // Fetch current track info
        fetch('/api/now-playing')
            .then(response => response.json())
            .then(data => {
                // Update UI
                if (currentTitle) currentTitle.textContent = data.title || 'Unknown Title';
                if (currentArtist) currentArtist.textContent = data.artist || 'Unknown Artist';
                if (currentAlbum) currentAlbum.textContent = data.album || 'Unknown Album';
                
                // Store track duration
                if (data.duration) {
                    state.trackDuration = data.duration;
                    state.trackPlaybackDuration = data.duration;
                }
                
                // Check for track change
                const newTrackId = data.path;
                if (state.currentTrackId && newTrackId && state.currentTrackId !== newTrackId) {
                    log(`Track changed on server: ${data.title}`, 'TRACK');
                    
                    // IMPORTANT: Flag track change handling is in progress
                    state.trackChangeInProgress = true;
                    
                    // Store the track change info for reference
                    state.currentTrackId = newTrackId;
                    state.lastTrackChange = Date.now();
                    
                    // Immediately restart the stream to get the new track
                    log(`Restarting stream for new track`, 'TRACK');
                    setTimeout(() => {
                        // Clear the flag when restarting
                        state.trackChangeInProgress = false;
                        restartDirectStreamWithImprovedBuffering();
                    }, 300);
                    
                    return; // Exit function to prevent further execution
                } else if (!state.currentTrackId && newTrackId) {
                    // First update
                    state.currentTrackId = newTrackId;
                }
                
                // Update server position tracking
                if (data.playback_position !== undefined) {
                    state.serverPosition = data.playback_position;
                    state.serverPositionTime = Date.now();
                    
                    // Check if we're far behind server
                    if (state.audioElement && !state.audioElement.paused && 
                        Math.abs(state.audioElement.currentTime - data.playback_position) > 10) {
                        log(`Client significantly behind server: ${state.audioElement.currentTime.toFixed(1)}s vs ${data.playback_position}s`, 'POSITION');
                        
                        // Only force sync if not near end of track
                        if (state.trackDuration > 0 && 
                            state.audioElement.currentTime < state.trackDuration * 0.9) {
                            log(`Forcing sync with server position`, 'POSITION');
                            forceSeekToServerPosition();
                        }
                    }
                }
                
                // Update listener count
                if (data.active_listeners !== undefined && listenerCount) {
                    const currentCount = listenerCount.textContent;
                    const newCount = `Listeners: ${data.active_listeners}`;
                    
                    if (currentCount !== newCount) {
                        listenerCount.textContent = newCount;
                    }
                }
                
                // Update page title
                document.title = `${data.title} - ${data.artist} | ChillOut Radio`;
            })
            .catch(error => {
                log(`Error fetching now playing: ${error.message}`, 'API', true);
                // Don't reconnect on API failures, just log them
            });
    }, pollingInterval);
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
                    startEnhancedBufferMonitoring();
                    startNowPlayingPolling();
                    
                    // Remove these listeners once successful
                    document.removeEventListener('click', tryPlayAudio);
                    document.removeEventListener('touchstart', tryPlayAudio);
                })
                .catch(e => {
                    log(`Still failed to play: ${e.message}`, 'AUDIO', true);
                    
                    // Show clear instructions
                    showStatus('Unable to start audio. Please try clicking the Connect button directly.', true);
                });
        }
    };
    
    // Add the listeners
    document.addEventListener('click', tryPlayAudio);
    document.addEventListener('touchstart', tryPlayAudio);
    
    // Highlight the play button
    if (startBtn) {
        startBtn.style.animation = 'pulse 1.5s infinite';
        
        // Remove animation when clicked
        startBtn.addEventListener('click', function removeAnimation() {
            startBtn.style.animation = '';
            startBtn.removeEventListener('click', removeAnimation);
        }, { once: true });
    }
}

// For backward compatibility (these functions are still used by other parts of the code)
function updateProgressBar(position, duration) {
    // This is now handled by updateProgressDisplay
    if (progressBar && duration > 0) {
        const percent = Math.min((position / duration) * 100, 100);
        progressBar.style.width = `${percent}%`;
        
        if (currentPosition) currentPosition.textContent = formatTime(position);
        if (currentDuration) currentDuration.textContent = formatTime(duration);
    }
}

// player-control.js - Part 6: Stream Restart and Recovery Functions (continued)

startPosition = Math.max(0, clientPosition - 2); // Back up slightly
log(`Resuming from client position: ${startPosition.toFixed(1)}s (server: ${serverPosition}s)`, 'POSITION');
} else if (clientPosition < serverPosition) {
// Our position is behind, use it to avoid skipping forward
startPosition = Math.max(0, clientPosition - 1);
log(`Resuming from client position: ${startPosition.toFixed(1)}s (behind server: ${serverPosition}s)`, 'POSITION');
} else {
// Too far ahead, use server position
startPosition = serverPosition;
log(`Client too far ahead (${clientPosition}s > ${serverPosition}s + ${maxAhead}s), using server position`, 'POSITION');
}
} else {
// Track changed, start from server position
log(`Track changed from ${state.currentTrackId} to ${trackId}, using server position: ${serverPosition}s`, 'POSITION');
startPosition = serverPosition;
}
}

// IMPORTANT: Start a bit before our target position to ensure smooth playback
const safePosition = Math.max(0, startPosition - 2);
log(`Restarting stream at position: ${safePosition.toFixed(1)}s (calculated from ${startPosition.toFixed(1)}s)`, 'CONTROL');

// Set loading state and update status
state.isLoading = true;
showStatus('Reconnecting...', false, false);

// Create URL with position
const timestamp = Date.now();

// Request larger buffer for reconnection attempts
const bufferSize = Math.min(90 + (state.reconnectAttempts * 15), 180); // Scale up buffer with reconnect attempts

const streamUrl = `/direct-stream?t=${timestamp}&position=${Math.floor(safePosition)}&track=${encodeURIComponent(trackId)}&buffer=${bufferSize}`;

// Set timeout for loading
if (state.loadTimeout) {
clearTimeout(state.loadTimeout);
}

state.loadTimeout = setTimeout(() => {
if (state.isLoading) {
log('Audio load timeout during restart - proceeding anyway', 'AUDIO', true);
state.isLoading = false;
proceedWithReconnection();
}
}, 6000);

// Set source and prepare to play
state.audioElement.src = streamUrl;

// Listen for data loaded
state.audioElement.addEventListener('loadeddata', function loadedHandler() {
state.audioElement.removeEventListener('loadeddata', loadedHandler);
log('Audio data loaded during restart', 'AUDIO');
state.isLoading = false;

if (state.loadTimeout) {
clearTimeout(state.loadTimeout);
state.loadTimeout = null;
}

proceedWithReconnection();
}, { once: true });

// Listen for canplay as fallback
state.audioElement.addEventListener('canplay', function canPlayHandler() {
state.audioElement.removeEventListener('canplay', canPlayHandler);

if (state.isLoading) {
log('Audio can play during restart', 'AUDIO');
state.isLoading = false;

if (state.loadTimeout) {
clearTimeout(state.loadTimeout);
state.loadTimeout = null;
}

proceedWithReconnection();
}
}, { once: true });

// Start loading
try {
state.audioElement.load();
} catch (e) {
log(`Error loading audio during restart: ${e.message}`, 'AUDIO', true);
state.isLoading = false;
proceedWithReconnection();
}

function proceedWithReconnection() {
// Update stream start time and position for progress calculation
state.streamStartTime = Date.now();
state.startPosition = safePosition;

// Clear any existing timers
clearAllTimers();

// Attempt to play with retries
playWithRetry(3);

function playWithRetry(retriesLeft) {
log(`Playing audio, ${retriesLeft} retries left`, 'AUDIO');

const playPromise = state.audioElement.play();

if (playPromise !== undefined) {
playPromise.then(() => {
    log('Stream restarted successfully', 'AUDIO');
    showStatus('Stream restarted');
    
    // Reset buffer underflows on successful restart
    if (state.reconnectAttempts <= 5) {
        state.bufferUnderflows = 0;
    }
    
    // Start monitoring and updating
    updateProgressDisplay();
    startEnhancedBufferMonitoring();
    startNowPlayingPolling();
    
    // Set up heartbeat check
    startHeartbeatChecks();
    
    // Force a seek to the correct position after a short delay
    setTimeout(forceSeekToServerPosition, 2000);
}).catch(e => {
    log(`Error playing after restart: ${e.message}`, 'AUDIO', true);
    
    if (e.name === 'NotAllowedError') {
        showStatus('Tap play button to start audio (browser requires user interaction)', true, false);
        setupUserInteractionHandlers();
    } else if (retriesLeft > 0) {
        log(`Retrying playback, ${retriesLeft} attempts left`, 'AUDIO');
        setTimeout(() => playWithRetry(retriesLeft - 1), 1000);
    } else {
        showStatus(`Playback error: ${e.message}`, true);
        
        // Back off with increasing delays between retries
        const backoffDelay = Math.min(2000 * Math.pow(1.5, state.reconnectAttempts), 15000);
        
        log(`Failed to restart after multiple attempts, will try again in ${(backoffDelay/1000).toFixed(1)}s`, 'CONTROL');
        setTimeout(restartDirectStreamWithImprovedBuffering, backoffDelay);
    }
});
} else {
// For older browsers that don't return a promise
log('Play method did not return a promise, assuming playback started', 'AUDIO');
showStatus('Stream restarted');

// Start monitoring and updating
updateProgressDisplay();
startEnhancedBufferMonitoring();
setTimeout(startNowPlayingPolling, 1000);

// Set up heartbeat check
startHeartbeatChecks();

// Force a seek to the correct position
setTimeout(forceSeekToServerPosition, 2000);
}
}
}
}).catch(error => {
log(`Error fetching track info for restart: ${error.message}`, 'API', true);

// Fallback restart without position info
const timestamp = Date.now();
const streamUrl = `/direct-stream?t=${timestamp}&buffer=120`; // Large buffer for fallback

state.streamStartTime = Date.now();
state.startPosition = 0;

if (state.audioElement) {
state.audioElement.src = streamUrl;

// Try to play with retry
state.audioElement.load();
state.audioElement.play().catch(e => {
log(`Error restarting stream: ${e.message}`, 'AUDIO', true);

if (e.name === 'NotAllowedError') {
showStatus('Tap play button to start audio (browser requires user interaction)', true, false);
setupUserInteractionHandlers();
} else {
// Retry after delay
setTimeout(() => {
    state.audioElement.play().catch(e2 => {
        log(`Second restart attempt failed: ${e2.message}`, 'AUDIO', true);
        stopDirectStream();
    });
}, 2000);
}
});
}

// Set up monitoring anyway
updateProgressDisplay();
startEnhancedBufferMonitoring();
startNowPlayingPolling();
startHeartbeatChecks();
});
}

// For slow networks, restart with smaller chunks
function restartWithSlowerNetworkSettings() {
log('Restarting with settings optimized for slower network', 'CONTROL');

if (!state.isPlaying) return;

// Record that we've optimized for slow network
state.optimizedForSlowNetwork = true;

// Restart with special flag for slower network
// Get current track position if possible
let currentPosition = 0;
if (state.audioElement && state.audioElement.currentTime > 0) {
currentPosition = Math.max(0, state.audioElement.currentTime - 2);
}

// Stop current playback
if (state.audioElement) {
try {
state.audioElement.pause();
state.audioElement.src = '';
state.audioElement.load();
} catch (e) {
log(`Error stopping playback for slow network restart: ${e}`, 'AUDIO');
}
}

// Fetch current track info
fetchNowPlaying().then(trackInfo => {
const trackId = trackInfo.path || 'unknown';

// Create specialized URL for slow networks
const timestamp = Date.now();
// Request smaller chunks (flag for server) and larger buffer
const streamUrl = `/direct-stream?t=${timestamp}&position=${Math.floor(currentPosition)}&track=${encodeURIComponent(trackId)}&buffer=120&slow=true`;

// Create new audio element
const newAudio = new Audio();
newAudio.volume = state.audioElement ? state.audioElement.volume : 0.7;
newAudio.muted = state.isMuted;
newAudio.setAttribute('playsinline', '');
newAudio.setAttribute('webkit-playsinline', '');
newAudio.setAttribute('preload', 'auto');

// Set up listeners
setupEnhancedAudioListeners();

// Set source
newAudio.src = streamUrl;
newAudio.load();

// Replace audio element
if (state.audioElement) {
state.audioElement.remove();
}
state.audioElement = newAudio;
document.body.appendChild(state.audioElement);

// Try to play
state.audioElement.play().catch(e => {
log(`Error starting playback for slow network: ${e.message}`, 'AUDIO', true);
// Fall back to normal restart
restartDirectStreamWithImprovedBuffering();
});

// Update state
state.streamStartTime = Date.now();
state.startPosition = currentPosition;

// Start monitoring
updateProgressDisplay();
startEnhancedBufferMonitoring();
startNowPlayingPolling();
startHeartbeatChecks();

// Force seek to correct position
setTimeout(forceSeekToServerPosition, 2000);
}).catch(error => {
log(`Error fetching track info for slow network: ${error.message}`, 'API', true);
// Fall back to normal restart
restartDirectStreamWithImprovedBuffering();
});
}

// Make functions available to other modules
window.startAudio = startAudio;
window.stopDirectStream = stopDirectStream;
window.toggleConnection = toggleConnection;
window.setupUserInteractionHandlers = setupUserInteractionHandlers;
window.setupEnhancedAudioListeners = setupEnhancedAudioListeners;
window.restartDirectStreamWithImprovedBuffering = restartDirectStreamWithImprovedBuffering;
window.startNowPlayingPolling = startNowPlayingPolling;
window.updateProgressDisplay = updateProgressDisplay;
window.updateProgressDisplayImmediate = updateProgressDisplayImmediate;
window.startEnhancedBufferMonitoring = startEnhancedBufferMonitoring;
window.getBufferInfo = getBufferInfo;
window.reportBufferState = reportBufferState;
window.forceSeekToServerPosition = forceSeekToServerPosition;
window.startHeartbeatChecks = startHeartbeatChecks;