// static/js/player.js - Updated with improved initialization

// player.js - Main entry point that loads all modules
// ChillOut Radio player - Updated with iOS compatibility and improved buffering

// Load order: when this file is included, it loads all modules in correct order
document.addEventListener('DOMContentLoaded', function() {
    // This file must be included last after all module files
    // Verify all modules are loaded
    if (typeof formatTime !== 'function' || 
        typeof startAudio !== 'function' || 
        typeof processQueue !== 'function' || 
        typeof connectWebSocket !== 'function' ||
        typeof checkMSECompatibility !== 'function' ||
        typeof boostInitialBuffer !== 'function') {
        
        console.error('[ERROR] Not all player modules are loaded. Please check script includes.');
        document.getElementById('status-message').textContent = 
            'Player initialization error. Please refresh the page.';
        document.getElementById('status-message').style.display = 'block';
        document.getElementById('status-message').style.borderLeftColor = '#e74c3c';
        return;
    }
    
    // Initialize player (function from player-core.js)
    initPlayer();
    
    // Detect platform for optimized experience
    const platform = detectPlatform();
    
    // Apply platform-specific optimizations
    optimizeMobileSettings();
    
    // Log initialization
    const platformInfo = platform.isIOS ? 
        'iOS detected (using direct stream)' : 
        (platform.isMobile ? 'Mobile platform detected' : 'Desktop platform detected');
    
    console.log(`ChillOut Radio player initialized - ${platformInfo}`);
    console.log('Buffering optimizations enabled for smoother playback');
});