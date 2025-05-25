// static/js/modular-player.js - Entry point for modular player

// Import all modules
import { mainPlayer } from './player/main-player.js';
import { iosStreamingManager } from './player/ios-manager.js';

// Export for global access if needed
window.ChillOutRadio = {
    player: mainPlayer,
    iosManager: iosManager,
    iosStreamingManager: iosStreamingManager,
    version: '2.1.1-ios-streaming-fixed'
};

// Log startup
console.log('%cChillOut Radio Player v2.1.1-ios-streaming-fixed loaded', 'color: #4CAF50; font-weight: bold; font-size: 14px;');
console.log('%cModular architecture with iOS streaming buffer management', 'color: #2196F3; font-style: italic;');