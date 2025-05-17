// static/js/player-control.js - Complete file

// Initialize and start playback
function startAudio() {
    log('Starting audio playback', 'CONTROL');
    startBtn.disabled = true;
    showStatus('Connecting to stream...', false, false);
    
    // Reset state
    state.reconnectAttempts = 0;
    state.audioQueue = [];
    state.consecutiveErrors = 0;
    state.lastAudioChunkTime = Date.now();
    state.lastTrackInfoTime = Date.now();
    
    // For iOS devices, use direct streaming approach
    if (state.isIOS) {
        log('Using direct streaming for iOS device', 'CONTROL');
        startDirectStream();
        return;
    }
    
    // Check browser support
    if (!('WebSocket' in window)) {
        showStatus('Your browser does not support WebSockets', true);
        startBtn.disabled = false;
        return;
    }
    
    // Regular MSE check
    if (!('MediaSource' in window)) {
        log('MediaSource API not supported, falling back to direct streaming', 'CONTROL');
        startDirectStream();
        return;
    }
    
    // Set up audio element
    if (!state.audioElement) {
        state.audioElement = new Audio();
        state.audioElement.controls = false;
        state.audioElement.volume = volumeControl.value;
        state.audioElement.muted = state.isMuted;
        state.audioElement.preload = 'auto';
        
        // Add to document but hide visually
        state.audioElement.style.display = 'none';
        document.body.appendChild(state.audioElement);
        
        // Set up audio event listeners
        setupAudioListeners();
    }
    
    // Standard MSE approach
    setupMediaSource();
    
    // Start connection health check timer
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
    }
    
    state.connectionHealthTimer = setInterval(checkConnectionHealth, 3000);
}

// Direct streaming implementation for iOS and browsers without MSE
function startDirectStream() {
    // Set flag so we know we're using direct stream mode
    state.usingDirectStream = true;
    state.isPlaying = true;
    
    // Create audio element if needed
    if (!state.audioElement) {
        state.audioElement = new Audio();
        state.audioElement.controls = false;
        state.audioElement.volume = volumeControl.value;
        state.audioElement.muted = state.isMuted;
        
        // Critical for iOS
        state.audioElement.setAttribute('playsinline', '');
        state.audioElement.setAttribute('webkit-playsinline', '');
        
        // Add to document but hide visually
        state.audioElement.style.display = 'none';
        document.body.appendChild(state.audioElement);
        
        // Set up basic audio listeners
        setupDirectStreamListeners();
    }
    
    // Create a direct stream URL with timestamp to prevent caching
    const timestamp = new Date().getTime();
    const streamUrl = `/direct-stream?t=${timestamp}`;
    
    log(`Connecting to direct stream: ${streamUrl}`, 'CONTROL');
    
    // Set the source 
    state.audioElement.src = streamUrl;
    
    // Try to play - this will likely require user interaction on iOS
    const playPromise = state.audioElement.play();
    
    // Handle play promise (modern browsers return a promise from play())
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
            
        }).catch(e => {
            log(`Error starting direct stream: ${e.message}`, 'AUDIO', true);
            
            if (e.name === 'NotAllowedError') {
                showStatus('Tap play button to start audio (iOS requires user interaction)', true, false);
                setupUserInteractionHandlers();
            } else {
                showStatus(`Playback error: ${e.message}`, true);
                stopDirectStream();
            }
            
            startBtn.disabled = false;
        });
    }
}

// Set up listeners specific to direct streaming
function setupDirectStreamListeners() {
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
            log('Audio ended unexpectedly, restarting', 'AUDIO', true);
            showStatus('Audio ended - reconnecting', true, false);
            restartDirectStream();
        }
    });
}

// Add helpers for iOS autoplay restrictions
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

// Poll for track info when using direct streaming
function startNowPlayingPolling() {
    // Clear any existing interval
    if (state.nowPlayingInterval) {
        clearInterval(state.nowPlayingInterval);
    }
    
    // Initial fetch
    fetchNowPlaying();
    
    // Set up polling every 5 seconds
    state.nowPlayingInterval = setInterval(() => {
        if (state.isPlaying) {
            fetchNowPlaying();
        } else {
            clearInterval(state.nowPlayingInterval);
            state.nowPlayingInterval = null;
        }
    }, 5000);
}

// Restart the direct stream if needed
function restartDirectStream() {
    if (!state.isPlaying) return;
    
    log('Restarting direct stream', 'CONTROL');
    
    // Create a new timestamp to avoid caching
    const timestamp = new Date().getTime();
    const streamUrl = `/direct-stream?t=${timestamp}`;
    
    // Stop the current playback
    state.audioElement.pause();
    state.audioElement.currentTime = 0;
    
    // Set new source and play
    state.audioElement.src = streamUrl;
    
    // Try to play
    const playPromise = state.audioElement.play();
    if (playPromise !== undefined) {
        playPromise.catch(e => {
            log(`Error restarting stream: ${e.message}`, 'AUDIO', true);
            
            if (e.name === 'NotAllowedError') {
                showStatus('Tap to restart audio', true, false);
                setupUserInteractionHandlers();
            }
        });
    }
}

// Stop direct streaming
function stopDirectStream() {
    log('Stopping direct stream', 'CONTROL');
    
    state.isPlaying = false;
    state.usingDirectStream = false;
    
    // Stop polling for track info
    if (state.nowPlayingInterval) {
        clearInterval(state.nowPlayingInterval);
        state.nowPlayingInterval = null;
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

// Stop audio playback and disconnect
function stopAudio(isError = false) {
    log(`Stopping audio playback${isError ? ' (due to error)' : ''}`, 'CONTROL');
    
    state.isPlaying = false;
    
    // If using direct streaming approach
    if (state.usingDirectStream) {
        stopDirectStream();
        return;
    }
    
    // Standard MSE cleanup
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
        state.connectionHealthTimer = null;
    }
    
    if (state.connectionTimeout) {
        clearTimeout(state.connectionTimeout);
        state.connectionTimeout = null;
    }
    
    // Close WebSocket
    if (state.ws) {
        try {
            state.ws.close();
        } catch (e) {
            // Ignore close errors
        }
        state.ws = null;
    }
    
    // Clean up MediaSource
    if (state.sourceBuffer) {
        state.sourceBuffer = null;
    }
    
    if (state.mediaSource && state.mediaSource.readyState === 'open') {
        try {
            state.mediaSource.endOfStream();
        } catch (e) {
            log(`Error ending MediaSource: ${e.message}`, 'MEDIA');
        }
    }
    state.mediaSource = null;
    
    // Stop audio
    if (state.audioElement) {
        state.audioElement.pause();
        state.audioElement.src = '';
        state.audioElement.load();
    }
    
    // Clear queue
    state.audioQueue = [];
    
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

// Make functions available to other modules
window.startAudio = startAudio;
window.stopAudio = stopAudio;
window.toggleConnection = toggleConnection;
window.startDirectStream = startDirectStream;
window.stopDirectStream = stopDirectStream;
window.setupUserInteractionHandlers = setupUserInteractionHandlers;
window.setupDirectStreamListeners = setupDirectStreamListeners;
window.restartDirectStream = restartDirectStream;
window.startNowPlayingPolling = startNowPlayingPolling;