// player-control.js - Playback control functions

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
    
    // Check browser support
    if (!('WebSocket' in window)) {
        showStatus('Your browser does not support WebSockets', true);
        startBtn.disabled = false;
        return;
    }
    
    if (!('MediaSource' in window)) {
        showStatus('Your browser does not support MediaSource', true);
        startBtn.disabled = false;
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
    
    // Set up MediaSource
    setupMediaSource();
    
    // Start connection health check timer
    if (state.connectionHealthTimer) {
        clearInterval(state.connectionHealthTimer);
    }
    state.connectionHealthTimer = setInterval(checkConnectionHealth, config.BUFFER_MONITOR_INTERVAL);
}

// Stop audio playback and disconnect
function stopAudio(isError = false) {
    log(`Stopping audio playback${isError ? ' (due to error)' : ''}`, 'CONTROL');
    
    state.isPlaying = false;
    
    // Clear all timers
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