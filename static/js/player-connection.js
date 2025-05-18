// player-connection.js - WebSocket connection and track info handling

// Process track info from WebSocket or API
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

// Handle WebSocket messages
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
            
            // Skip empty buffers
            if (buffer.byteLength === 0) {
                return;
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
            
            // Check message type
            if (data.type === 'now_playing') {
                updateTrackInfo(data.track);
            } else {
                // Default treatment as track info
                updateTrackInfo(data);
            }
        } catch (e) {
            log(`Non-JSON message: ${event.data}`, 'STREAM');
        }
    }
}

// Improved connection health monitoring
function checkConnectionHealth() {
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
    } else {
        // Log buffer state for monitoring
        if (bufferHealth.ahead < config.AUDIO_STARVATION_THRESHOLD) {
            log(`WARNING: Low buffer - ${bufferHealth.ahead.toFixed(1)}s ahead, ${state.audioQueue.length} chunks queued`, 'HEALTH');
        } else if (state.debugMode) {
            log(`Buffer health: ${bufferHealth.ahead.toFixed(1)}s ahead, ${state.audioQueue.length} chunks queued`, 'HEALTH');
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
        updateTrackInfo(data);
    } catch (error) {
        log(`Error fetching now playing: ${error.message}`, 'API', true);
    }
}

// Improved WebSocket connection with better error handling
function connectWebSocket() {
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
        // Determine WebSocket URL
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/stream`;
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
            
            // Start audio playback
            if (state.audioElement.paused) {
                const playPromise = state.audioElement.play();
                playPromise.catch(e => {
                    log(`Play error: ${e.message}`, 'AUDIO', true);
                    if (e.name === 'NotAllowedError') {
                        showStatus('Click play to start audio (browser requires user interaction)', true, false);
                    }
                });
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
                }, 1000);
            }
        };
        
        state.ws.onerror = (error) => {
            log('WebSocket error', 'STREAM', true);
            
            // Don't immediately try to reconnect - wait for the close event
            showStatus('Connection error', true, false);
        };
        
        state.ws.onmessage = handleWebSocketMessage;
        
        // Set connection timeout (increased for slower connections)
        state.connectionTimeout = setTimeout(() => {
            if (state.ws && state.audioQueue.length === 0) {
                log('Connection timeout - no data received', 'STREAM', true);
                showStatus('Connection timeout. Reconnecting...', true, false);
                attemptReconnection();
            }
        }, 20000);
        
    } catch (e) {
        log(`WebSocket setup error: ${e.message}`, 'STREAM', true);
        showStatus(`Connection error: ${e.message}`, true);
        attemptReconnection();
    }
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
    const baseDelay = Math.min(500 * Math.pow(1.3, state.reconnectAttempts - 1), 8000);
    const jitter = Math.random() * 1000; // Add up to 1 second of jitter
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