// Updated player.js for direct streaming

// player.js - Main entry point that loads all modules
// ChillOut Radio player - Updated for Direct Streaming on all platforms

// Load order: when this file is included, it loads all modules in correct order
document.addEventListener('DOMContentLoaded', function() {
    // This file must be included last after all module files
    // Verify all modules are loaded
    if (typeof formatTime !== 'function' || 
        typeof startAudio !== 'function' || 
        typeof fetchNowPlaying !== 'function' ||
        typeof handleTrackInfoUpdate !== 'function' ||
        typeof updateProgressBar !== 'function') {
        
        console.error('[ERROR] Not all player modules are loaded. Please check script includes.');
        document.getElementById('status-message').textContent = 
            'Player initialization error. Please refresh the page.';
        document.getElementById('status-message').style.display = 'block';
        document.getElementById('status-message').style.borderLeftColor = '#e74c3c';
        return;
    }
    
    // Initialize player (function from player-core.js)
    initPlayer();
    
    // Detect platform for logging purposes
    const platform = detectPlatform();
    
    // Log initialization
    const platformInfo = platform.isIOS ? 
        'iOS device detected' : 
        (platform.isMobile ? 'Mobile device detected' : 'Desktop device detected');
    
    console.log(`ChillOut Radio player initialized - ${platformInfo}`);
    console.log('Using direct HTTP streaming for all platforms');
});