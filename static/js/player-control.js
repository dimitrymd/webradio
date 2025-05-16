// player-control.js update - Support iOS playback control

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
    
    // Compatibility check for MediaSource
    const mseSupport = checkMSECompatibility();
    if (!mseSupport.supported) {
        showStatus(`Media error: ${mseSupport.message}`, true);
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
        
        // iOS-specific audio attributes
        if (state.isIOS) {
            // These attributes help with iOS audio playback
            state.audioElement.setAttribute('playsinline', '');
            state.audioElement.setAttribute('webkit-playsinline', '');
            state.audioElement.setAttribute('autoplay', '');
        }
        
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
    
    // iOS devices need more frequent health checks
    const healthCheckInterval = state.isIOS ? 
        config.BUFFER_MONITOR_INTERVAL / 2 : // More frequent for iOS
        config.BUFFER_MONITOR_INTERVAL;
        
    state.connectionHealthTimer = setInterval(checkConnectionHealth, healthCheckInterval);
    
    // Special handling for iOS
    if (state.isIOS) {
        // Create a user interaction event handler to start audio on iOS
        // iOS requires user interaction to start audio
        const unlockAudio = () => {
            log('User interaction detected - attempting to unlock audio', 'AUDIO');
            
            // Try to play audio
            if (state.audioElement && state.audioElement.paused) {
                state.audioElement.play()
                    .then(() => {
                        log('Audio unlocked on iOS', 'AUDIO');
                    })
                    .catch(e => {
                        log(`Failed to unlock audio: ${e.message}`, 'AUDIO', true);
                    });
            }
            
            // Remove the event listeners once we've tried to unlock
            document.removeEventListener('touchstart', unlockAudio);
            document.removeEventListener('touchend', unlockAudio);
            document.removeEventListener('click', unlockAudio);
        };
        
        // Add interaction event listeners
        document.addEventListener('touchstart', unlockAudio, { once: true });
        document.addEventListener('touchend', unlockAudio, { once: true });
        document.addEventListener('click', unlockAudio, { once: true });
        
        // Show a message to instruct the user
        showStatus('Tap anywhere to enable audio on iOS', false, false);
    }
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
        
        // For iOS devices, we need to use a different approach
        if (state.isIOS) {
            log('Starting audio for iOS device', 'CONTROL');
            
            // iOS sometimes needs user interaction to start audio
            // This may now be handled by the touchstart/click handlers in startAudio()
            // but we'll keep this explicit approach too
            const startAudioWithUserInteraction = () => {
                startAudio();
                document.removeEventListener('click', startAudioWithUserInteraction);
            };
            
            // Add a one-time click handler to start audio
            document.addEventListener('click', startAudioWithUserInteraction, { once: true });
            
            // Also directly start audio (this might work if we already had user interaction)
            startAudio();
        } else {
            // Non-iOS devices can just start directly
            startAudio();
        }
    }
}