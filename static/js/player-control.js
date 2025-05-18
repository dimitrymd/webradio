// player-control.js - Part 1: Main Audio Control Functions

function startAudio() {
    log('Starting audio playback via direct streaming', 'CONTROL');
    startBtn.disabled = true;
    showStatus('Connecting to stream...', false, false);
    
    // Reset state
    state.reconnectAttempts = 0;
    state.bufferUnderflows = 0;
    state.lastBufferAttempt = 0;
    state.bufferMetrics = [];
    state.bufferPauseActive = false;
    state.initialStartTime = Date.now();
    
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
        
        // BUFFERING IMPROVEMENT: Start from slightly earlier position (2s) and increase buffer
        const timestamp = Date.now();
        // Start a bit earlier to prevent gaps - with safety check
        const positionSec = Math.max(0, Math.min(serverPosition - 2, (state.trackDuration || 300) - 10)); 
        
        // ENHANCED: Request larger buffer (90s) to reduce pauses during playback
        const streamUrl = `/direct-stream?t=${timestamp}&position=${positionSec}&track=${encodeURIComponent(trackId)}&buffer=90`;
        
        log(`Connecting to direct stream: ${streamUrl}`, 'CONTROL');
        
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
                        showStatus('Streaming started');
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
                
                // Start progress calculation right away
                updateProgressDisplay();
                
                // Start enhanced buffer monitoring
                startEnhancedBufferMonitoring();
                
                // Start polling with a small delay
                setTimeout(() => {
                    startNowPlayingPolling();
                }, 2000);
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
    });
}

// player-control.js - Part 3: Event Listeners for Audio Element

// Enhanced audio event listeners with better buffer management
function setupEnhancedAudioListeners() {
    // For performance, use passive event listeners where appropriate
    const passiveOpts = { passive: true };
    
    state.audioElement.addEventListener('playing', () => {
        log(`Audio playing`, 'AUDIO');
        showStatus('Audio playing');
        
        // When we first start playing, update the display
        updateProgressDisplay();
        
        // Reset buffer underflows on successful playback
        setTimeout(() => {
            if (state.audioElement && !state.audioElement.paused) {
                state.bufferUnderflows = 0;
            }
        }, 5000);
    }, passiveOpts);
    
    // ENHANCED: Better buffering detection and handling
    state.audioElement.addEventListener('waiting', () => {
        log('Audio buffering - waiting for data', 'AUDIO');
        showStatus('Buffering...', false, false);
        
        // Track buffer starvation with rate limiting
        const now = Date.now();
        if (now - state.lastBufferEvent > 2000) {
            state.bufferUnderflows++;
            state.lastBufferEvent = now;
            
            // If we've had too many buffer underflows in a short time, take action
            if (state.bufferUnderflows > 5 && now - state.streamStartTime < 30000) {
                // Multiple underflows in first 30 seconds - likely network issues
                log('Multiple buffer underflows at start - network may be too slow', 'AUDIO', true);
                
                // Try to adjust audio element options for slower networks
                try {
                    // Request smaller chunk size by restarting
                    restartWithSlowerNetworkSettings();
                    return;
                } catch (e) {
                    log(`Error adjusting for slow network: ${e.message}`, 'AUDIO', true);
                }
            }
        }
        
        // Try to recover from buffer underflow with adaptive playback rate
        if (state.audioElement && state.isPlaying) {
            try {
                const oldRate = state.audioElement.playbackRate;
                
                // Progressively slow down based on underflow frequency
                const newRate = Math.max(0.8, 1.0 - (state.bufferUnderflows * 0.03));
                
                if (oldRate !== newRate) {
                    state.audioElement.playbackRate = newRate;
                    log(`Adjusted playback rate to ${newRate.toFixed(2)} for buffering recovery`, 'AUDIO');
                    
                    // Schedule playback rate restoration after buffer improves
                    clearTimeout(state.rateRestoreTimeout);
                    state.rateRestoreTimeout = setTimeout(() => {
                        if (state.audioElement && state.isPlaying) {
                            // Gradually restore to normal
                            const current = state.audioElement.playbackRate;
                            const newValue = Math.min(1.0, current + 0.05);
                            state.audioElement.playbackRate = newValue;
                            log(`Gradually restoring playback rate to ${newValue.toFixed(2)}`, 'AUDIO');
                            
                            // Continue restoring if needed
                            if (newValue < 1.0) {
                                state.rateRestoreTimeout = setTimeout(arguments.callee, 5000);
                            }
                        }
                    }, 10000);
                }
            } catch (e) {
                // Ignore playback rate errors
            }
        }
    });
    
    state.audioElement.addEventListener('stalled', () => {
        log('Audio stalled - network issue?', 'AUDIO');
        showStatus('Stream stalled - buffering', true, false);
        
        // If we stall for too long, attempt to reconnect
        if (state.stallTimeout) {
            clearTimeout(state.stallTimeout);
        }
        
        // Progressive timeout based on previous issues
        const stallTimeoutDuration = state.bufferUnderflows > 3 ? 15000 : 8000;
        
        state.stallTimeout = setTimeout(() => {
            if (state.isPlaying && state.audioElement && 
                (state.audioElement.paused || state.audioElement.readyState < 3)) {
                log(`Stall timeout after ${stallTimeoutDuration/1000}s - attempting to restart`, 'AUDIO', true);
                restartDirectStreamWithImprovedBuffering();
            }
        }, stallTimeoutDuration);
    });
    
    // ENHANCED: Better progress event monitoring
    state.audioElement.addEventListener('progress', () => {
        // Clear stall timeout if we're making progress
        if (state.stallTimeout) {
            clearTimeout(state.stallTimeout);
            state.stallTimeout = null;
        }
        
        // Check buffer state occasionally
        if (Math.random() < 0.05) { // ~5% of events
            reportBufferState();
        }
    }, passiveOpts);
    
    // Enhanced error handling with detailed diagnosis
    state.audioElement.addEventListener('error', (e) => {
        const error = state.audioElement.error;
        const errorCode = error ? error.code : 'unknown';
        const errorMessage = error ? error.message : 'Unknown error';
        
        log(`Audio error (code ${errorCode}): ${errorMessage}`, 'AUDIO', true);
        
        // Provide meaningful error messages based on code
        let errorDescription = "Audio error";
        switch (errorCode) {
            case 1: // MEDIA_ERR_ABORTED
                errorDescription = "Playback aborted";
                break;
            case 2: // MEDIA_ERR_NETWORK
                errorDescription = "Network error";
                break;
            case 3: // MEDIA_ERR_DECODE
                errorDescription = "Audio decoding error";
                break;
            case 4: // MEDIA_ERR_SRC_NOT_SUPPORTED
                errorDescription = "Audio format not supported";
                break;
        }
        
        // Only attempt recovery if we're trying to play
        if (state.isPlaying) {
            showStatus(`${errorDescription} - attempting to recover`, true, false);
            
            // Network errors benefit most from a restart
            if (errorCode === 2) {
                // Use progressive backoff based on number of errors
                const delay = Math.min(1000 * Math.pow(1.5, state.reconnectAttempts), 8000);
                
                setTimeout(() => {
                    restartDirectStreamWithImprovedBuffering();
                }, delay);
            } else if (errorCode === 3) {
                // For decode errors, try to adjust position
                try {
                    if (state.audioElement.currentTime > 0) {
                        log('Trying to skip past decoding error', 'AUDIO');
                        state.audioElement.currentTime = state.audioElement.currentTime + 1;
                        
                        // Try to resume after position change
                        setTimeout(() => {
                            if (state.audioElement && state.isPlaying) {
                                state.audioElement.play().catch(e => {
                                    log(`Failed to resume after position change: ${e.message}`, 'AUDIO', true);
                                    restartDirectStreamWithImprovedBuffering();
                                });
                            }
                        }, 500);
                    } else {
                        // If at beginning, just restart
                        restartDirectStreamWithImprovedBuffering();
                    }
                } catch (e) {
                    log(`Error handling decode error: ${e.message}`, 'AUDIO', true);
                    restartDirectStreamWithImprovedBuffering();
                }
            } else {
                // For other errors, just restart
                restartDirectStreamWithImprovedBuffering();
            }
        }
    });
    
    // Improved end of track detection and handling
    state.audioElement.addEventListener('ended', () => {
        log('Audio ended event fired', 'AUDIO');
        
        // If we're still supposed to be playing, handle track end properly
        if (state.isPlaying) {
            // Get current position estimate and track duration
            const startTime = state.streamStartTime || 0;
            const startPosition = state.startPosition || 0;
            const elapsedSec = (Date.now() - startTime) / 1000;
            const estimatedPosition = startPosition + elapsedSec;
            const duration = state.trackDuration || 0;
            
            log(`Ended event at estimated position: ${estimatedPosition.toFixed(1)}s of ${duration}s`, 'AUDIO');
            
            // More intelligent track end detection
            const timeSinceStart = (Date.now() - state.streamStartTime) / 1000;
            const realEndExpected = (duration > 0 && estimatedPosition > (duration * 0.85));
            const minPlaybackTime = Math.min(30, duration * 0.3); // At least 30s or 30% of duration
            
            if (realEndExpected && timeSinceStart > minPlaybackTime) {
                // This is likely a real track end - get updated track info
                log('Track appears to have ended normally, checking for next track', 'AUDIO');
                showStatus('Track ended, loading next...', false, false);
                
                // Force an immediate track info update to check for new track
                fetchNowPlaying().then(trackInfo => {
                    if (trackInfo && trackInfo.path && trackInfo.path !== state.currentTrackId) {
                        // Track has changed - restart with new track
                        log('New track detected, restarting stream', 'TRACK');
                        setTimeout(restartDirectStreamWithImprovedBuffering, 500);
                    } else {
                        // Same track or no change yet - try seeking to beginning
                        log('Same track still playing on server, seeking to beginning', 'TRACK');
                        try {
                            // Seek to beginning of current track
                            state.audioElement.currentTime = 0;
                            state.audioElement.play().catch(e => {
                                log(`Failed to restart track: ${e.message}`, 'AUDIO', true);
                                restartDirectStreamWithImprovedBuffering();
                            });
                        } catch (e) {
                            log(`Error seeking to beginning: ${e.message}`, 'AUDIO', true);
                            restartDirectStreamWithImprovedBuffering();
                        }
                    }
                }).catch(error => {
                    log(`Error fetching track info: ${error.message}, restarting stream`, 'API', true);
                    setTimeout(restartDirectStreamWithImprovedBuffering, 1000);
                });
            } else {
                // This is likely a false end - probably a buffer issue
                log('False end detected - track did not complete. Restarting stream.', 'AUDIO', true);
                showStatus('Reconnecting...', true, false);
                
                // Check audio element state for diagnostics
                const readyState = state.audioElement.readyState;
                const buffered = state.audioElement.buffered;
                let bufferInfo = "No buffer";
                
                if (buffered && buffered.length > 0) {
                    bufferInfo = `Buffer: ${buffered.start(0)}-${buffered.end(0)}s`;
                }
                
                log(`Audio state at false end: readyState=${readyState}, ${bufferInfo}`, 'AUDIO');
                
                // Restart with slight delay
                setTimeout(restartDirectStreamWithImprovedBuffering, 1000);
            }
        }
    });
    
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

// player-control.js - Part 5: Track Info Polling and "Now Playing" Updates

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
                    
                    // Handle track change based on playback position
                    const position = state.audioElement ? state.audioElement.currentTime : 0;
                    const duration = state.trackDuration || 0;
                    const percentage = duration > 0 ? position / duration : 0;
                    
                    // Store the track change info for reference
                    state.currentTrackId = newTrackId;
                    state.lastTrackChange = Date.now();
                    
                    // Only restart if already near the end of the previous track
                    // OR if we've been playing for a long time and might be out of sync
                    const playingFor = (Date.now() - state.streamStartTime) / 1000;
                    
                    if (percentage > 0.85 || playingFor > 300) { // >85% through or over 5 minutes
                        log(`Restarting stream for new track (position: ${percentage.toFixed(2)}, time played: ${playingFor.toFixed(0)}s)`, 'TRACK');
                        setTimeout(restartDirectStreamWithImprovedBuffering, 500);
                    } else {
                        log(`Track changed on server but we're only at ${(percentage*100).toFixed(0)}%, continuing playback`, 'TRACK');
                    }
                } else if (!state.currentTrackId && newTrackId) {
                    // First update
                    state.currentTrackId = newTrackId;
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

// For backward compatibility (these functions are no longer used)
function updateProgressBar(position, duration) {
    // This is now handled by updateProgressDisplay
    if (progressBar && duration > 0) {
        const percent = Math.min((position / duration) * 100, 100);
        progressBar.style.width = `${percent}%`;
        
        if (currentPosition) currentPosition.textContent = formatTime(position);
        if (currentDuration) currentDuration.textContent = formatTime(duration);
    }
}

// player-control.js - Part 6: Stream Restart and Recovery Functions

// Restart stream with improved buffering
function restartDirectStreamWithImprovedBuffering() {
    log('Restarting stream with improved buffering', 'CONTROL');
    
    if (!state.isPlaying) {
        log('Not restarting - playback stopped', 'CONTROL');
        return;
    }
    
    // Increment reconnect attempts
    state.reconnectAttempts++;
    
    // Limit maximum restart attempts to prevent endless loops
    if (state.reconnectAttempts > 10) {
        const timeSinceStart = (Date.now() - (state.initialStartTime || 0)) / 1000;
        
        // If we've been trying for a long time (>5 minutes), allow more restarts
        if (timeSinceStart < 300) {
            log(`Too many restart attempts (${state.reconnectAttempts}), stopping reconnection loop`, 'CONTROL', true);
            showStatus('Too many reconnection attempts. Please try again later.', true);
            stopDirectStream();
            return;
        } else {
            // Reset counter after 5 minutes to allow new attempts
            log('Resetting reconnection counter after extended playback time', 'CONTROL');
            state.reconnectAttempts = 1;
        }
    }
    
    // Properly clean up old audio element
    if (state.audioElement) {
        try {
            // Keep volume setting
            const volume = state.audioElement.volume;
            const muted = state.audioElement.muted;
            
            // Clean up
            state.audioElement.pause();
            state.audioElement.src = '';
            state.audioElement.load();
            state.audioElement.remove();
            
            // Create new element with same settings
            state.audioElement = new Audio();
            state.audioElement.volume = volume;
            state.audioElement.muted = muted;
            state.audioElement.setAttribute('playsinline', '');
            state.audioElement.setAttribute('webkit-playsinline', '');
            state.audioElement.setAttribute('preload', 'auto');
            
            // Add to DOM
            state.audioElement.style.display = 'none';
            document.body.appendChild(state.audioElement);
            
            // Set up listeners
            setupEnhancedAudioListeners();
        } catch (e) {
            log(`Error cleaning up audio element: ${e.message}`, 'AUDIO', true);
            // Create a fresh audio element anyway
            state.audioElement = new Audio();
            state.audioElement.style.display = 'none';
            document.body.appendChild(state.audioElement);
            setupEnhancedAudioListeners();
        }
    }
    
    // Get current track info before restarting
    fetchNowPlaying().then((trackInfo) => {
        // Get server position
        const serverPosition = trackInfo.playback_position || 0;
        
        // Track ID for continuity
        const trackId = trackInfo.path || 'unknown';
        
        // Update track info
        state.trackDuration = trackInfo.duration || 0;
        state.currentTrackId = trackId;
        
        // Determine optimal start position
        // For track changes or errors, start from current server position
        // For buffer issues, try to resume close to where we left off
        let startPosition = serverPosition;
        
        if (state.audioElement && state.audioElement.currentTime > 0) {
            // If we were playing and this is a buffer/network recovery,
            // try to resume close to where we were
            if (trackId === state.currentTrackId) {
                // Same track - use our position but ensure it's not too far ahead
                // of server position to avoid getting out of sync
                const clientPosition = state.audioElement.currentTime;
                const maxAhead = 10; // Don't get more than 10 seconds ahead
                
                if (clientPosition > serverPosition && clientPosition - serverPosition < maxAhead) {
                    // Our position is reasonable, use it
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
window.startEnhancedBufferMonitoring = startEnhancedBufferMonitoring;
window.getBufferInfo = getBufferInfo;
window.reportBufferState = reportBufferState;