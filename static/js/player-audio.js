// static/js/player-audio.js - Complete file

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

// Get current buffer health metrics
function getBufferHealth() {
    if (!state.sourceBuffer || !state.audioElement || state.sourceBuffer.buffered.length === 0) {
        return {
            current: 0,
            ahead: 0,
            duration: 0,
            underflow: true
        };
    }
    
    const currentTime = state.audioElement.currentTime;
    const bufferedEnd = state.sourceBuffer.buffered.end(state.sourceBuffer.buffered.length - 1);
    const bufferAhead = bufferedEnd - currentTime;
    const totalBuffered = state.sourceBuffer.buffered.end(state.sourceBuffer.buffered.length - 1) - 
                         state.sourceBuffer.buffered.start(0);
    
    return {
        current: currentTime,
        ahead: bufferAhead,
        duration: totalBuffered,
        underflow: bufferAhead < config.AUDIO_STARVATION_THRESHOLD
    };
}

// Process audio data queue
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
    
    // If we have a very high buffer, slow down processing
    if (bufferHealth.ahead > config.TARGET_BUFFER_SIZE * 1.5 && queueSizeInChunks > 5) {
        // We have plenty of buffer, so delay processing to avoid excess memory usage
        setTimeout(processQueue, 100);
        return;
    }
    
    try {
        // Get data from queue
        const data = state.audioQueue.shift();
        state.sourceBuffer.appendBuffer(data);
        state.lastAudioChunkTime = Date.now();
        
        // Reset consecutive errors since we successfully processed data
        state.consecutiveErrors = 0;
        
        // Log buffer status occasionally
        if (queueSizeInChunks % 50 === 0 || bufferHealth.underflow) {
            log(`Buffer health: ${bufferHealth.ahead.toFixed(1)}s ahead, ${queueSizeInChunks} chunks queued`, 'BUFFER');
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
                    // Normal processing with a small delay to reduce CPU load
                    setTimeout(processQueue, 5);  // Small delay instead of 0
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
            // For other errors, try again soon with backoff
            const retryDelay = Math.min(50 * state.consecutiveErrors, 1000);
            setTimeout(processQueue, retryDelay);
            
            // If we've had many consecutive errors, try recreation
            if (state.consecutiveErrors > 5) {
                log('Too many consecutive errors, recreating MediaSource', 'BUFFER', true);
                recreateMediaSource();
            }
        }
    }
}

// Handle quota exceeded errors with smarter buffer management
function handleQuotaExceededError() {
    try {
        if (state.sourceBuffer && state.sourceBuffer.buffered.length > 0) {
            const currentTime = state.audioElement.currentTime;
            
            // Only remove data that's definitely been played
            const safeRemovalPoint = Math.max(
                state.sourceBuffer.buffered.start(0),
                currentTime - 2  // Keep 2 seconds before current position
            );
            
            // Calculate how much we need to remove
            const removalEnd = Math.min(
                safeRemovalPoint + 5,  // Remove 5 seconds of audio
                currentTime - 1  // But never too close to current playback position
            );
            
            if (removalEnd > safeRemovalPoint) {
                log(`Clearing buffer segment ${safeRemovalPoint.toFixed(1)}-${removalEnd.toFixed(1)}s`, 'BUFFER');
                state.sourceBuffer.remove(safeRemovalPoint, removalEnd);
                
                // Continue after buffer clear
                state.sourceBuffer.addEventListener('updateend', function onClearEnd() {
                    state.sourceBuffer.removeEventListener('updateend', onClearEnd);
                    setTimeout(processQueue, 50);
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
    
    log('Recreating MediaSource', 'MEDIA');
    
    try {
        // Preserve some audio data to continue playback
        const savedQueue = state.audioQueue.slice(-50); // Keep only the last 50 chunks
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
                
                // Restore queue and continue
                state.audioQueue = savedQueue;
                state.consecutiveErrors = 0;
                setTimeout(processQueue, 100);
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

// Set up audio element event listeners
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
    
    // Add new timeupdate listener to monitor buffer health dynamically
    state.audioElement.addEventListener('timeupdate', () => {
        // Skip for direct streaming mode
        if (state.usingDirectStream) return;
        
        // Check buffer health on time updates (but not too frequently - skip most updates)
        if (Math.random() < 0.05) { // Only check ~5% of time updates to reduce overhead
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
}

// Set up MediaSource with error handling
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
                
                // Add buffer monitoring event
                state.sourceBuffer.addEventListener('updateend', () => {
                    // Check how much we've buffered after each update
                    if (state.sourceBuffer && state.sourceBuffer.buffered.length > 0 && state.audioElement) {
                        const bufferHealth = getBufferHealth();
                        
                        // If buffer is getting very large, trim it
                        if (bufferHealth.duration > config.MAX_BUFFER_SIZE) {
                            const currentTime = state.audioElement.currentTime;
                            const trimPoint = Math.max(state.sourceBuffer.buffered.start(0), currentTime - 10);
                            log(`Trimming buffer: ${trimPoint.toFixed(2)}s to current time - 10`, 'BUFFER');
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