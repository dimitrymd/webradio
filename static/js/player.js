// player.js - Improved entry point with better module loading
// ChillOut Radio player - Direct Streaming Implementation

// Load order: this file loads all modules in the correct order
document.addEventListener('DOMContentLoaded', function() {
    // This file must be included last after all module files
    
    // Version and build info for debugging
    const PLAYER_VERSION = '1.2.0';
    const BUILD_DATE = '2025-05-18';
    
    // Make sure state and config objects exist
    if (typeof window.state === 'undefined') {
        console.error('[ERROR] State object not initialized. Ensure player-core.js is loaded first.');
        
        // Create minimal state object to prevent further errors
        window.state = {
            debugMode: true,
            errorHistory: [],
            isPlaying: false,
            performanceMetrics: { bufferSamples: 0, avgBufferSize: 0, lastBufferCheck: 0 }
        };
    }
    
    if (typeof window.config === 'undefined') {
        console.error('[ERROR] Config object not initialized. Ensure player-core.js is loaded first.');
        
        // Create minimal config object to prevent further errors
        window.config = {
            MIN_BUFFER_SIZE: 2,
            TARGET_BUFFER_SIZE: 10,
            MAX_BUFFER_SIZE: 30,
            AUDIO_STARVATION_THRESHOLD: 0.5
        };
    }
    
    // Initialize UI elements if not already done
    if (!startBtn) {
        startBtn = document.getElementById('start-btn');
        muteBtn = document.getElementById('mute-btn');
        volumeControl = document.getElementById('volume');
        progressBar = document.getElementById('progress-bar');
        currentPosition = document.getElementById('current-position');
        currentDuration = document.getElementById('current-duration');
        currentTitle = document.getElementById('current-title');
        currentArtist = document.getElementById('current-artist');
        currentAlbum = document.getElementById('current-album');
        listenerCount = document.getElementById('listener-count');
        statusMessage = document.getElementById('status-message');
    }
    
    // Verify all modules are loaded
    const requiredFunctions = [
        'formatTime',
        'showStatus',
        'startAudio',
        'fetchNowPlaying',
        'handleTrackInfoUpdate',
        'updateProgressDisplay'
    ];
    
    const missingFunctions = requiredFunctions.filter(
        fn => typeof window[fn] !== 'function'
    );
    
    if (missingFunctions.length > 0) {
        console.error('[ERROR] Missing required player modules:', missingFunctions.join(', '));
        console.error('[ERROR] Please check script includes. Required files:');
        console.error('- player-core.js');
        console.error('- player-api.js');
        console.error('- player-control.js');
        
        // Show error in UI as well
        const statusEl = document.getElementById('status-message');
        if (statusEl) {
            statusEl.textContent = 'Player initialization error. Please refresh the page.';
            statusEl.style.display = 'block';
            statusEl.style.borderLeftColor = '#e74c3c';
        }
        return;
    }
    
    // Initialize player (function from player-core.js)
    initPlayer();
    
    // Log startup information
    console.log(`ChillOut Radio player v${PLAYER_VERSION} (${BUILD_DATE})`);
    
    // For debugging - expose state object to console in development
    if (window.location.hostname === 'localhost' || window.location.hostname === '127.0.0.1') {
        window._playerState = state;
        window._playerConfig = config;
        console.log('Development mode: Player state and config accessible via _playerState and _playerConfig');
    }
    
    // Listen for network status changes
    if ('connection' in navigator && navigator.connection) {
        navigator.connection.addEventListener('change', function() {
            const connection = navigator.connection;
            const type = connection.effectiveType || 'unknown';
            const downlink = connection.downlink || 'unknown';
            
            log(`Network changed: ${type}, downlink: ${downlink} Mbps`, 'NETWORK');
            
            // If connection deteriorates during playback, consider adjusting settings
            if (state.isPlaying && 
                (type === 'slow-2g' || type === '2g' || (downlink && downlink < 1))) {
                log('Connection deteriorated during playback, optimizing for slower network', 'NETWORK');
                
                // Don't restart immediately, but mark for optimization on next restart
                state.optimizedForSlowNetwork = true;
            }
        });
    }
    
    // Add custom error handler for uncaught errors
    window.addEventListener('error', function(event) {
        log(`Uncaught error: ${event.message} at ${event.filename}:${event.lineno}`, 'ERROR', true);
        
        // If related to audio playback, try to recover
        if (event.message.includes('audio') || 
            event.message.includes('media') || 
            event.filename.includes('player-')) {
            
            log('Attempting recovery from uncaught player error', 'ERROR');
            
            // Only restart if we're supposed to be playing
            if (state.isPlaying && window.restartDirectStreamWithImprovedBuffering) {
                setTimeout(window.restartDirectStreamWithImprovedBuffering, 2000);
            }
        }
    });
});