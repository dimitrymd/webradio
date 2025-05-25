// static/js/player/config.js - Player configuration and constants

export const CONFIG = {
    NOW_PLAYING_INTERVAL: 10000,        // Check every 10 seconds (battery friendly)
    CONNECTION_CHECK_INTERVAL: 8000,    // Check connection every 8 seconds
    RECONNECT_ATTEMPTS: 5,              // Reduced attempts for mobile
    DEBUG_MODE: true,
    
    // Mobile-friendly error handling
    MAX_ERROR_FREQUENCY: 8000,          // Longer time between error responses
    CLEANUP_DELAY: 500,                 // Longer cleanup delay for mobile
    RECONNECT_MIN_DELAY: 2000,          // Longer minimum delay
    RECONNECT_MAX_DELAY: 15000,         // Longer maximum delay
    
    // Track transition optimized for mobile
    TRACK_CHANGE_GRACE_PERIOD: 3000,    // Longer grace period
    POSITION_SYNC_TOLERANCE: 5,         // More lenient tolerance for mobile
    POSITION_SAVE_INTERVAL: 8000,       // Less frequent saving for battery
    
    // Mobile-specific timeouts
    MOBILE_BUFFER_TIMEOUT: 12000,       // Longer buffer timeout
    MOBILE_HEARTBEAT_INTERVAL: 15000,   // Heartbeat to keep connection alive
    STALE_CONNECTION_TIMEOUT: 30000,    // When to consider connection stale
    
    // iOS-specific settings
    IOS_RECONNECT_ATTEMPTS: 8,          // More attempts for iOS
    IOS_BACKGROUND_HEARTBEAT: 45000,    // Background heartbeat interval
    IOS_VISIBILITY_DELAY: 1000,         // Delay before checking after visibility change
    IOS_MEMORY_CHECK_INTERVAL: 30000,   // Check memory pressure every 30s
    IOS_CONNECTION_WATCHDOG: 20000,     // Connection health check every 20s
};

export const PLATFORM = {
    isIOS: /iPad|iPhone|iPod/.test(navigator.userAgent) && !window.MSStream,
    isSafari: /^((?!chrome|android).)*safari/i.test(navigator.userAgent),
    isMobile: /Mobi|Android/i.test(navigator.userAgent),
    isAndroid: /Android/i.test(navigator.userAgent),
    androidVersion: navigator.userAgent.match(/Android (\d+)/)?.[1] || 'unknown',
};

export const AUDIO_CONFIG = {
    CROSSORIGIN: "anonymous",
    PRELOAD_MOBILE: 'metadata',
    PRELOAD_DESKTOP: 'auto',
    DEFAULT_VOLUME: 0.7,
    
    // Platform-specific settings
    IOS_PLAYSINLINE: true,
    ANDROID_WEBKIT_PLAYSINLINE: true,
    MOBILE_AUTOPLAY: false,
    DESKTOP_AUTOPLAY: false,
};

export const NETWORK_CONFIG = {
    SLOW_2G_MULTIPLIER: 2.0,
    SLOW_3G_MULTIPLIER: 1.5,
    GOOD_CONNECTION_THRESHOLD: 2.0, // Mbps
    
    // Timeouts by network type
    TIMEOUTS: {
        'slow-2g': {
            NOW_PLAYING: 20000,
            BUFFER_TIMEOUT: 25000,
            RECONNECT_MIN: 5000,
        },
        '2g': {
            NOW_PLAYING: 20000,
            BUFFER_TIMEOUT: 25000,
            RECONNECT_MIN: 5000,
        },
        '3g': {
            NOW_PLAYING: 15000,
            BUFFER_TIMEOUT: 18000,
            RECONNECT_MIN: 3000,
        },
        '4g': {
            NOW_PLAYING: 10000,
            BUFFER_TIMEOUT: 12000,
            RECONNECT_MIN: 2000,
        }
    }
};

export const API_ENDPOINTS = {
    NOW_PLAYING: '/api/now-playing',
    HEARTBEAT: '/api/heartbeat',
    STREAM_STATUS: '/stream-status',
    DIRECT_STREAM: '/direct-stream',
    ANDROID_POSITION: '/api/android-position',
    SYNC_CHECK: '/api/sync-check',
    STATS: '/api/stats',
    HEALTH: '/api/health',
};

export const ERROR_CODES = {
    MEDIA_ERR_ABORTED: 1,
    MEDIA_ERR_NETWORK: 2,
    MEDIA_ERR_DECODE: 3,
    MEDIA_ERR_SRC_NOT_SUPPORTED: 4,
};

export const CONNECTION_STATES = {
    DISCONNECTED: 'disconnected',
    CONNECTING: 'connecting',
    CONNECTED: 'connected',
    RECONNECTING: 'reconnecting',
    ERROR: 'error',
};