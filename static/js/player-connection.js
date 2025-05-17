// static/js/player-connection.js - Updated with better buffering and error handling

// Improved WebSocket connection with better error handling
function connectWebSocket() {
    // Skip for direct streaming mode
    if (state.usingDirectStream) return;
    
    // Clean up any existing connection
    if (state.ws) {
        try {
            state.ws.close();
        } catch (e) {
            // Ignore close errors
        }
        state.ws = null;
    }
    
    try {
        // Determine WebSocket URL based on platform
        const wsUrl = getWebSocketURL();
        log(`Connecting to WebSocket: ${wsUrl}`, 'STREAM');
        
        // Create connection
        state.ws = new WebSocket(wsUrl);
        
        // Set up event handlers
        state.ws.onopen = () => {
            log('WebSocket connection established', 'STREAM');
            showStatus('Connected to stream');
            startBtn.textContent = 'Disconnect';
            startBtn.disabled = false;
            startBtn.dataset.connected = 'true';
            state.isPlaying = true;
            
            // Reset reconnect attempts on successful connection
            state.reconnectAttempts = 0;
            
            // Request initial track info
            try {
                state.ws.send(JSON.stringify({ type: 'now_playing_request' }));
                log('Requested initial track info', 'TRACK');
            } catch (e) {
                log(`Error requesting track info: ${e.message}`, 'TRACK', true);
                // Fallback to API
                fetchNowPlaying();
            }
            
            // Start audio playback with boost for initial buffering
            if (state.audioElement.paused) {
                // First wait to build up some buffer
                setTimeout(() => {
                    const playPromise = state.audioElement.play();
                    playPromise.catch(e => {
                        log(`Play error: ${e.message}`, 'AUDIO', true);
                        if (e.name === 'NotAllowedError') {
                            showStatus('Click play to start audio (browser requires user interaction)', true, false);
                        }
                    });
                    
                    // After playback starts, boost buffer
                    playPromise.then(() => {
                        boostInitialBuffer();
                    });
                }, 100);
            }
        };
        
        state.ws.onclose = (event) => {
            log(`WebSocket closed: Code ${event.code}`, 'STREAM');
            
            // Only attempt reconnect if we're still supposed to be playing
            if (state.isPlaying) {
                // Use a brief delay to avoid hammering the server
                setTimeout(() => {
                    if (state.isPlaying) {
                        showStatus('Connection closed. Reconnecting...', true, false);
                        attemptReconnection();
                    }
                }, 500); // Reduced from 1000ms to 500ms
            }
        };
        
        state.ws.onerror = (error) => {
            log('WebSocket error', 'STREAM', true);
            
            // Don't immediately try to reconnect - wait for the close event
            showStatus('Connection error', true, false);
        };
        
        state.ws.onmessage = handleWebSocketMessage;
        
        // Set connection timeout (increased for slower connections)
        const timeoutDuration = 20000;
        state.connectionTimeout = setTimeout(() => {
            if (state.ws && state.audioQueue.length === 0) {
                log('Connection timeout - no data received', 'STREAM', true);
                showStatus('Connection timeout. Reconnecting...', true, false);
                attemptReconnection();
            }
        }, timeoutDuration);
        
    } catch (e) {
        log(`WebSocket setup error: ${e.message}`, 'STREAM', true);
        showStatus(`Connection error: ${e.message}`, true);
        attemptReconnection();
    }
}

// Handle WebSocket messages with improved buffering
function handleWebSocketMessage(event) {
    // Clear connection timeout if set
    if (state.connectionTimeout) {
        clearTimeout(state.connectionTimeout);
        state.connectionTimeout = null;
    }
    
    // Process binary audio data
    if (event.data instanceof Blob) {
        // Handle non-text data (audio)
        event.data.arrayBuffer().then(buffer => {
            // Skip empty buffers
            if (buffer.byteLength === 0) {
                return;
            }
            
            // Check for special markers (2-byte control messages)
            if (buffer.byteLength === 2) {
                const view = new Uint8Array(buffer);
                if (view[0] === 0xFF && view[1] === 0xFE) {
                    log('Track transition marker received', 'STREAM');
                    // For track transitions, just keep processing
                    return;
                }
                if (view[0] === 0xFF && view[1] === 0xFF) {
                    log('Track end marker received', 'STREAM');
                    return;
                }
            }
            
            // Add data to queue
            state.audioQueue.push(buffer);
            state.lastAudioChunkTime = Date.now();
            
            // Start processing if not already going
            processQueue();
            
        }).catch(e => {
            log(`Error processing binary data: ${e.message}`, 'STREAM', true);
        });
    } else {
        // Process text data (track info or other commands)
        try {
            const data = JSON.parse(event.data);
            handleTrackInfoUpdate(data);
        } catch (e) {
            log(`Non-JSON message: ${event.data}`, 'STREAM');
        }
    }
}

// Enhanced track info handling for mid-stream joins and position info
function handleTrackInfoUpdate(info) {
    try {
        // Check for error message
        if (info.error) {
            showStatus(`Server error: ${info.error}`, true);
            return;
        }
        
        // Check if this is a mid-stream join
        const midStreamJoin = info.mid_stream_join === true;
        const position = info.playback_position || 0;
        const percentage = info.percentage || 0;
        
        // Store track ID for change detection
        const newTrackId = info.path;
        if (state.currentTrackId !== newTrackId) {
            log(`Track changed to: ${info.title}`, 'TRACK');
            state.currentTrackId = newTrackId;
            
            // Reset position tracking
            state.lastKnownPosition = 0;
        }
        
        // Update UI
        if (currentTitle) currentTitle.textContent = info.title || 'Unknown Title';
        if (currentArtist) currentArtist.textContent = info.artist || 'Unknown Artist';
        if (currentAlbum) currentAlbum.textContent = info.album || 'Unknown Album';
        
        // Update progress
        if (info.duration) {
            if (currentDuration) currentDuration.textContent = formatTime(info.duration);
            
            // When joining mid-stream, immediately set the progress bar
            if (midStreamJoin && percentage > 0) {
                // Update progress bar to current position
                updateProgressBar(position, info.duration);
                
                // Store last known position
                state.lastKnownPosition = position;
                
                log(`Joined stream at position ${position}s (${percentage}%)`, 'TRACK');
            }
        }
        
        if (info.playback_position !== undefined) {
            state.lastKnownPosition = info.playback_position;
            updateProgressBar(info.playback_position, info.duration);
        }
        
        // Update listener count
        if (info.active_listeners !== undefined && listenerCount) {
            listenerCount.textContent = `Listeners: ${info.active_listeners}`;
        }
        
        // Store track ID in DOM for future comparison
        if (currentTitle) currentTitle.dataset.trackId = info.path;
        
        // Update page title
        document.title = `${info.title} - ${info.artist} | ChillOut Radio`;
        
        // Update last track info time
        state.lastTrackInfoTime = Date.now();
    } catch (e) {
        log(`Error processing track info: ${e.message}`, 'TRACK', true);
    }
}

// Improved connection health monitoring
function checkConnectionHealth() {
    // Skip for direct streaming mode
    if (state.usingDirectStream) return;
    
    if (!state.isPlaying) return;
    
    const now = Date.now();
    const timeSinceLastAudio = (now - state.lastAudioChunkTime) / 1000;
    const timeSinceLastTrackInfo = (now - state.lastTrackInfoTime) / 1000;
    
    // Get buffer metrics
    const bufferHealth = getBufferHealth();
    
    // Check if we've received audio data recently
    if (timeSinceLastAudio > config.NO_DATA_TIMEOUT) {
        log(`No audio data received for ${timeSinceLastAudio.toFixed(1)}s`, 'HEALTH', true);
        
        // If buffer is also getting low, we need to reconnect
        if (bufferHealth.ahead < config.AUDIO_STARVATION_THRESHOLD) {
            log('Buffer depleted and no new data, reconnecting', 'HEALTH', true);
            showStatus('Connection interrupted. Reconnecting...', true, false);
            attemptReconnection();
        } else {
            // We still have buffer, so playback can continue
            // Send a ping to see if connection is still alive
            if (state.ws && state.ws.readyState === WebSocket.OPEN) {
                try {
                    state.ws.send(JSON.stringify({ type: 'ping' }));
                    log('Sent ping to check connection', 'HEALTH');
                } catch (e) {
                    log(`Error sending ping: ${e.message}`, 'HEALTH', true);
                }
            }
        }
    }
    
    // Check if we need to request now playing info
    if (timeSinceLastTrackInfo > config.NOW_PLAYING_INTERVAL / 1000) {
        // Request now playing update via WebSocket
        if (state.ws && state.ws.readyState === WebSocket.OPEN) {
            try {
                state.ws.send(JSON.stringify({ type: 'now_playing_request' }));
                log('Requested now playing info via WebSocket', 'TRACK');
            } catch (e) {
                log(`Error requesting now playing: ${e.message}`, 'TRACK', true);
                // Fallback to API
                fetchNowPlaying();
            }
        } else {
            // Fallback to API if WebSocket not available
            fetchNowPlaying();
        }
    }
    
    // Check buffer health periodically
    if (now - state.performanceMetrics.lastBufferCheck > 10000) { // Every 10 seconds
        log(`Buffer health check: ${bufferHealth.ahead.toFixed(1)}s ahead, ${state.audioQueue.length} chunks queued, ${state.bufferUnderflows} underflows`, 'HEALTH');
        state.performanceMetrics.lastBufferCheck = now;
    }
}

// Fetch now playing info via API (fallback method)
async function fetchNowPlaying() {
    try {
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            log(`Now playing API error: ${response.status}`, 'API', true);
            return;
        }
        
        const data = await response.json();
        handleTrackInfoUpdate(data);
    } catch (error) {
        log(`Error fetching now playing: ${error.message}`, 'API', true);
    }
}

// Improved reconnection with reduced backoff for faster recovery
function attemptReconnection() {
    // Skip for direct streaming mode
    if (state.usingDirectStream) return;
    
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
    
    // Calculate delay with more gentle exponential backoff and less jitter
    const baseDelay = Math.min(config.RECONNECT_DELAY_BASE * 
                         Math.pow(config.RECONNECT_BACKOFF_FACTOR, state.reconnectAttempts - 1), 
                         5000); // Cap at 5 seconds
    const jitter = Math.random() * 500; // Reduced jitter
    const delay = baseDelay + jitter;
    
    log(`Reconnection attempt ${state.reconnectAttempts}/${state.maxReconnectAttempts} in ${(delay/1000).toFixed(1)}s`, 'CONTROL');
    showStatus(`Reconnecting (${state.reconnectAttempts}/${state.maxReconnectAttempts})...`, true, false);
    
    // Close existing connection
    if (state.ws) {
        try {
            state.ws.close();
        } catch (e) {
            // Ignore close errors
        }
        state.ws = null;
    }
    
    // Schedule reconnection
    setTimeout(() => {
        if (state.isPlaying) {
            // Set up a fresh MediaSource
            setupMediaSource();
        }
    }, delay);
}

// Make functions available to other modules
window.connectWebSocket = connectWebSocket;
window.handleWebSocketMessage = handleWebSocketMessage;
window.handleTrackInfoUpdate = handleTrackInfoUpdate;
window.checkConnectionHealth = checkConnectionHealth;
window.fetchNowPlaying = fetchNowPlaying;
window.attemptReconnection = attemptReconnection;