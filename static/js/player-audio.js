// static/js/player-audio.js - Updated for better buffering

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

// Improved buffer health metrics for better diagnostics
function getBufferHealth() {
    if (!state.sourceBuffer || !state.audioElement || state.sourceBuffer.buffered.length === 0) {
        return {
            current: 0,
            ahead: 0,
            duration: 0,
            underflow: true,
            utilization: 0
        };
    }
    
    const currentTime = state.audioElement.currentTime;
    const bufferedEnd = state.sourceBuffer.buffered.end(state.sourceBuffer.buffered.length - 1);
    const bufferAhead = bufferedEnd - currentTime;
    const totalBuffered = state.sourceBuffer.buffered.end(state.sourceBuffer.buffered.length - 1) - 
                         state.sourceBuffer.buffered.start(0);
    
    // Calculate buffer utilization percentage
    const utilization = (bufferAhead / config.MAX_BUFFER_SIZE) * 100;
    
    // Track buffer health for performance metrics
    state.lastBufferHealth = bufferAhead;
    
    // Add to performance metrics
    state.performanceMetrics.avgBufferSize = 
        (state.performanceMetrics.avgBufferSize * state.performanceMetrics.bufferSamples + bufferAhead) / 
        (state.performanceMetrics.bufferSamples + 1);
    state.performanceMetrics.bufferSamples += 1;
    
    return {
        current: currentTime,
        ahead: bufferAhead,
        duration: totalBuffered,
        underflow: bufferAhead < config.AUDIO_STARVATION_THRESHOLD,
        utilization: utilization
    };
}

// Improved process queue function for more efficient buffering
function processQueue() {
    // Skip for direct streaming mode
    if (state.usingDirectStream) return;
    
    // Exit conditions - ensure all necessary components are ready
    if (state.audioQueue.length === 0 || !state.sourceBuffer || !state.mediaSource || 
        state.mediaSource.readyState !== 'open' || state.sourceBuffer.updating) {
        return;
    }
    
    // Check buffer health before processing
    const bufferHealth = getBufferHealth();
    const queueSizeInChunks = state.audioQueue.length;
    
    // If we have a very high buffer, slow down processing but be less aggressive
    if (bufferHealth.ahead > config.TARGET_BUFFER_SIZE * 2 && queueSizeInChunks > 10) {
        // We have plenty of buffer, so delay processing to avoid excess memory usage
        setTimeout(processQueue, 200);  // Increased from 100ms to 200ms
        return;
    }
    
    try {
        // Get data from queue
        const data = state.audioQueue.shift();
        state.sourceBuffer.appendBuffer(data);
        state.lastAudioChunkTime = Date.now();
        
        // Reset consecutive errors since we successfully processed data
        state.consecutiveErrors = 0;
        
        // Log buffer status occasionally or if there's a potential issue
        if (queueSizeInChunks % 50 === 0 || bufferHealth.underflow) {
            log(`Buffer health: ${bufferHealth.ahead.toFixed(1)}s ahead, ${queueSizeInChunks} chunks queued, utilization: ${bufferHealth.utilization.toFixed(1)}%`, 'BUFFER');
        }
        
        // Set up callback for when this append completes
        state.sourceBuffer.addEventListener('updateend', function onUpdateEnd() {
            state.sourceBuffer.removeEventListener('updateend', onUpdateEnd);
            
            // Continue processing queue with adaptive scheduling
            if (state.audioQueue.length > 0) {
                // Adjust timing based on buffer health
                if (bufferHealth.ahead < config.MIN_BUFFER_SIZE) {
                    // Buffer is low, process next chunk immediately
                    processQueue();
                } else {
                    // Normal processing with reduced delay
                    setTimeout(processQueue, 1);  // Minimal delay
                }
            }
        }, { once: true });
        
    } catch (e) {
        log(`Error processing audio data: ${e.message}`, 'BUFFER', true);
        state.consecutiveErrors++;
        
        // Handle different error types
        if (e.name === 'QuotaExceededError') {
            // More strategic buffer management for quota errors
            handleQuotaExceededError();
        } else {
            // For other errors, try again soon with reduced backoff
            const retryDelay = Math.min(25 * state.consecutiveErrors, 500); // Reduced max delay
            setTimeout(processQueue, retryDelay);
            
            // If we've had many consecutive errors, try recreation
            if (state.consecutiveErrors > 5) {
                log('Too many consecutive errors, recreating MediaSource', 'BUFFER', true);
                recreateMediaSource();
            }
        }
    }
}

// Improved quota exceeded error handling
function handleQuotaExceededError() {
    try {
        if (state.sourceBuffer && state.sourceBuffer.buffered.length > 0) {
            const currentTime = state.audioElement.currentTime;
            
            // Only remove data that's definitely been played
            const safeRemovalPoint = Math.max(
                state.sourceBuffer.buffered.start(0),
                currentTime - 5  // Keep 5 seconds before current position (increased from 2)
            );
            
            // Calculate how much we need to remove
            const removalEnd = Math.min(
                safeRemovalPoint + 10,  // Remove 10 seconds of audio (increased from 5)
                currentTime - 1  // But never too close to current playback position
            );
            
            if (removalEnd > safeRemovalPoint) {
                log(`Clearing buffer segment ${safeRemovalPoint.toFixed(1)}-${removalEnd.toFixed(1)}s to free memory`, 'BUFFER');
                state.sourceBuffer.remove(safeRemovalPoint, removalEnd);
                
                // Continue after buffer clear
                state.sourceBuffer.addEventListener('updateend', function onClearEnd() {
                    state.sourceBuffer.removeEventListener('updateend', onClearEnd);
                    setTimeout(processQueue, 10); // Reduced delay (was 50ms)
                }, { once: true });
                return;
            }
        }
        
        // If we couldn't clear the buffer using the approach above
        log('Could not clear buffer, trying alternative approach', 'BUFFER', true);
        recreateMediaSource();
    } catch (clearError) {
        log(`Error clearing buffer: ${clearError.message}`, 'BUFFER', true);
        recreateMediaSource();
    }
}

// Recreate the MediaSource to recover from serious errors
function recreateMediaSource() {
    // Skip for direct streaming mode
    if (state.usingDirectStream) return;
    
    log('Recreating MediaSource to recover from errors', 'MEDIA');
    
    try {
        // Preserve more audio data to continue playback
        const savedQueue = state.audioQueue.slice(-100); // Keep more chunks (increased from 50)
        state.audioQueue = []; // Clear the queue
        
        // Clean up old MediaSource
        if (state.sourceBuffer) {
            state.sourceBuffer = null;
        }
        
        if (state.mediaSource && state.mediaSource.readyState === 'open') {
            try {
                state.mediaSource.endOfStream();
            } catch (e) {
                // Ignore errors during cleanup
            }
        }
        
        // Create new MediaSource
        state.mediaSource = new MediaSource();
        
        state.mediaSource.addEventListener('sourceopen', function onSourceOpen() {
            log('New MediaSource opened', 'MEDIA');
            
            try {
                // Create source buffer with the appropriate type
                const mimeType = getSourceBufferType();
                log(`Creating source buffer with MIME type: ${mimeType}`, 'MEDIA');
                
                state.sourceBuffer = state.mediaSource.addSourceBuffer(mimeType);
                
                // Setup error handler
                state.sourceBuffer.addEventListener('error', (event) => {
                    log(`SourceBuffer error: ${event.message || 'Unknown error'}`, 'MEDIA', true);
                });
                
                // Restore queue and continue with minimal delay
                state.audioQueue = savedQueue;
                state.consecutiveErrors = 0;
                setTimeout(processQueue, 50);
            } catch (e) {
                log(`Error creating source buffer: ${e.message}`, 'MEDIA', true);
                attemptReconnection();
            }
        });
        
        // Connect to audio element
        const url = URL.createObjectURL(state.mediaSource);
        state.audioElement.src = url;
        
        // Make sure we're playing
        if (state.audioElement.paused && state.isPlaying) {
            state.audioElement.play().catch(e => {
                log(`Error playing after recreation: ${e.message}`, 'MEDIA', true);
            });
        }
    } catch (e) {
        log(`Error recreating MediaSource: ${e.message}`, 'MEDIA', true);
        // If recreation fails, attempt reconnection as a last resort
        attemptReconnection();
    }
}

// Enhanced audio element event listeners with better mobile support
function setupAudioListeners() {
    state.audioElement.addEventListener('playing', () => {
        log('Audio playing', 'AUDIO');
        showStatus('Audio playing');
    });
    
    state.audioElement.addEventListener('waiting', () => {
        log('Audio buffering', 'AUDIO');
        showStatus('Buffering...', false, false);
        
        // On buffer starvation, immediately process queue if possible
        if (state.audioQueue.length > 0 && !state.sourceBuffer.updating) {
            processQueue(); // Process queue immediately when buffer starves
        }
        
        // Track buffer underflows for diagnostics
        state.bufferUnderflows++;
    });
    
    state.audioElement.addEventListener('stalled', () => {
        log('Audio stalled', 'AUDIO');
        showStatus('Stream stalled - buffering', true, false);
    });
    
    state.audioElement.addEventListener('error', (e) => {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        log(`Audio error (code ${errorCode})`, 'AUDIO', true);
        
        // Only react to errors if we're still trying to play
        if (state.isPlaying) {
            // Don't react to errors too frequently
            const now = Date.now();
            if (now - state.lastErrorTime > 5000) { // Reduced from 10 seconds to 5 seconds
                state.lastErrorTime = now;
                showStatus('Audio error - attempting to recover', true, false);
                
                // Try recreating the MediaSource
                recreateMediaSource();
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
    
    // Add new timeupdate listener to monitor buffer health more frequently
    state.audioElement.addEventListener('timeupdate', () => {
        // Skip for direct streaming mode
        if (state.usingDirectStream) return;
        
        // Check buffer health on time updates (increased frequency)
        if (Math.random() < 0.1) { // Increased from 0.05 to 0.1 (10% of updates)
            const bufferHealth = getBufferHealth();
            if (bufferHealth.underflow) {
                log(`Buffer underfull during playback: ${bufferHealth.ahead.toFixed(2)}s ahead`, 'AUDIO');
                // Process queue immediately if we have data
                if (state.audioQueue.length > 0 && !state.sourceBuffer.updating) {
                    processQueue();
                }
            }
        }
    });
    
    // Optimize for mobile
    if (state.isMobile) {
        log('Setting up mobile-optimized audio listeners', 'AUDIO');
        
        // More aggressive stalled/waiting event handling for mobile
        state.audioElement.addEventListener('waiting', () => {
            log('Mobile device audio waiting, immediate buffer check', 'AUDIO');
            if (state.audioQueue.length > 0 && !state.sourceBuffer.updating) {
                processQueue(); // Process queue immediately
            }
        }, { passive: true }); // Using passive event for better performance
        
        // Add playbackRate adjustment when buffers are low on mobile
        setInterval(() => {
            if (state.audioElement && !state.audioElement.paused) {
                const bufferHealth = getBufferHealth();
                if (bufferHealth.ahead < config.MIN_BUFFER_SIZE) {
                    // Low buffer, slow down playback slightly to build buffer
                    state.audioElement.playbackRate = 0.97;
                } else {
                    // Normal buffer, use almost normal speed
                    state.audioElement.playbackRate = 0.99;
                }
            }
        }, 2000);
    }
}

// Set up MediaSource with improved error handling and buffering
function setupMediaSource() {
    try {
        // Create MediaSource
        state.mediaSource = new MediaSource();
        
        // Set up event handlers
        state.mediaSource.addEventListener('sourceopen', () => {
            log('MediaSource opened', 'MEDIA');
            
            try {
                // Create source buffer with the appropriate MIME type
                const mimeType = getSourceBufferType();
                log(`Creating source buffer with MIME type: ${mimeType}`, 'MEDIA');
                
                state.sourceBuffer = state.mediaSource.addSourceBuffer(mimeType);
                
                // Add improved buffer monitoring
                state.sourceBuffer.addEventListener('updateend', () => {
                    // Check how much we've buffered after each update
                    if (state.sourceBuffer && state.sourceBuffer.buffered.length > 0 && state.audioElement) {
                        const bufferHealth = getBufferHealth();
                        
                        // If buffer is getting very large, trim it but keep more data
                        if (bufferHealth.duration > config.MAX_BUFFER_SIZE) {
                            const currentTime = state.audioElement.currentTime;
                            const trimPoint = Math.max(state.sourceBuffer.buffered.start(0), currentTime - 20); // Keep 20 seconds behind
                            log(`Trimming buffer: ${trimPoint.toFixed(2)}s to current time - 20s (was too large)`, 'BUFFER');
                            try {
                                state.sourceBuffer.remove(state.sourceBuffer.buffered.start(0), trimPoint);
                            } catch (e) {
                                log(`Error trimming buffer: ${e.message}`, 'BUFFER');
                            }
                        }
                    }
                });
                
                // Connect to WebSocket after MediaSource is ready
                connectWebSocket();
            } catch (e) {
                log(`Error creating source buffer: ${e.message}`, 'MEDIA', true);
                showStatus(`Media error: ${e.message}`, true);
                startBtn.disabled = false;
            }
        });
        
        state.mediaSource.addEventListener('sourceended', () => log('MediaSource ended', 'MEDIA'));
        state.mediaSource.addEventListener('sourceclose', () => log('MediaSource closed', 'MEDIA'));
        
        // Create object URL and set as audio source
        const url = URL.createObjectURL(state.mediaSource);
        state.audioElement.src = url;
        
    } catch (e) {
        log(`MediaSource setup error: ${e.message}`, 'MEDIA', true);
        showStatus(`Media error: ${e.message}`, true);
        startBtn.disabled = false;
    }
}

// Make functions available to other modules
window.processQueue = processQueue;
window.getBufferHealth = getBufferHealth;
window.updateProgressBar = updateProgressBar;
window.setupMediaSource = setupMediaSource;
window.recreateMediaSource = recreateMediaSource;
window.setupAudioListeners = setupAudioListeners;
window.handleQuotaExceededError = handleQuotaExceededError;