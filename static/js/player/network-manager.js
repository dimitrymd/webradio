// static/js/player/network-manager.js - Network and API management

import { CONFIG, API_ENDPOINTS, NETWORK_CONFIG } from './config.js';
import { playerState } from './state.js';
import { logger } from './logger.js';

export class NetworkManager {
    constructor() {
        this.setupNetworkMonitoring();
        this.setupOnlineOfflineHandlers();
    }
    
    setupNetworkMonitoring() {
        if (navigator.connection) {
            const updateNetworkInfo = () => {
                const connection = navigator.connection;
                const oldType = playerState.networkType;
                playerState.networkType = connection.effectiveType || 'unknown';
                
                if (oldType !== playerState.networkType) {
                    logger.log(`Network changed: ${oldType} -> ${playerState.networkType}`, 'NETWORK');
                    this.adaptToNetworkQuality();
                    this.emit('networkChanged', { 
                        from: oldType, 
                        to: playerState.networkType 
                    });
                }
            };
            
            navigator.connection.addEventListener('change', updateNetworkInfo);
            updateNetworkInfo(); // Initial check
        }
    }
    
    setupOnlineOfflineHandlers() {
        window.addEventListener('online', () => {
            logger.log('Network connection restored', 'NETWORK');
            this.emit('networkRestored');
        });
        
        window.addEventListener('offline', () => {
            logger.error('Network connection lost', 'NETWORK');
            this.emit('networkLost');
        });
    }
    
    adaptToNetworkQuality() {
        const timeouts = NETWORK_CONFIG.TIMEOUTS[playerState.networkType];
        
        if (timeouts) {
            // Update configuration based on network type
            CONFIG.NOW_PLAYING_INTERVAL = timeouts.NOW_PLAYING;
            CONFIG.MOBILE_BUFFER_TIMEOUT = timeouts.BUFFER_TIMEOUT;
            CONFIG.RECONNECT_MIN_DELAY = timeouts.RECONNECT_MIN;
            
            logger.log(`Adapted to ${playerState.networkType} network:`, 'NETWORK');
            logger.log(`- Now playing interval: ${timeouts.NOW_PLAYING}ms`, 'NETWORK');
            logger.log(`- Buffer timeout: ${timeouts.BUFFER_TIMEOUT}ms`, 'NETWORK');
            logger.log(`- Reconnect delay: ${timeouts.RECONNECT_MIN}ms`, 'NETWORK');
        }
    }
    
    assessConnectionQuality() {
        const metrics = {
            rtt: 0,
            downlink: 0,
            effectiveType: 'unknown'
        };
        
        if (navigator.connection) {
            const conn = navigator.connection;
            metrics.rtt = conn.rtt || 0;
            metrics.downlink = conn.downlink || 0;
            metrics.effectiveType = conn.effectiveType || 'unknown';
        }
        
        // Calculate quality score (0-100)
        let quality = 100;
        
        if (metrics.rtt > 0) {
            if (metrics.rtt > 1000) quality -= 40;
            else if (metrics.rtt > 500) quality -= 20;
            else if (metrics.rtt > 200) quality -= 10;
        }
        
        if (metrics.downlink > 0) {
            if (metrics.downlink < 0.5) quality -= 30;
            else if (metrics.downlink < 1) quality -= 15;
            else if (metrics.downlink < 2) quality -= 5;
        }
        
        switch (metrics.effectiveType) {
            case 'slow-2g':
                quality = Math.min(quality, 20);
                break;
            case '2g':
                quality = Math.min(quality, 40);
                break;
            case '3g':
                quality = Math.min(quality, 70);
                break;
        }
        
        return {
            score: Math.max(0, quality),
            metrics: metrics,
            rating: quality >= 80 ? 'excellent' : 
                    quality >= 60 ? 'good' : 
                    quality >= 40 ? 'fair' : 'poor'
        };
    }
    
    // API Methods
    async fetchNowPlaying() {
        try {
            logger.log("Fetching now playing information", 'API');
            
            let apiUrl = API_ENDPOINTS.NOW_PLAYING;
            if (playerState.isPlatformMobile) {
                apiUrl += '?mobile_client=true';
            }
            
            const response = await fetch(apiUrl, {
                headers: {
                    'Cache-Control': 'no-cache'
                }
            });
            
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}`);
            }
            
            const data = await response.json();
            logger.log(`Received track info: ${data.title || 'Unknown'}`, 'API');
            
            return data;
        } catch (error) {
            logger.error(`Error fetching now playing: ${error.message}`, 'API');
            throw error;
        }
    }
    
    async sendHeartbeat() {
        if (!playerState.isPlaying || !playerState.connectionId) return;
        
        try {
            const response = await fetch(`${API_ENDPOINTS.HEARTBEAT}?connection_id=${playerState.connectionId}`, {
                method: 'GET',
                headers: {
                    'Cache-Control': 'no-cache'
                }
            });
            
            if (response.ok) {
                playerState.lastHeartbeat = Date.now();
                
                const data = await response.json();
                if (data.active_listeners !== undefined) {
                    this.emit('listenerCountUpdated', data.active_listeners);
                }
                
                logger.log(`Heartbeat sent successfully`, 'HEARTBEAT');
            }
        } catch (error) {
            logger.log(`Heartbeat failed: ${error.message}`, 'HEARTBEAT');
        }
    }
    
    async fetchAndroidPosition() {
        try {
            logger.log("Android: Fetching server-authoritative position", 'ANDROID');
            
            const response = await fetch(API_ENDPOINTS.ANDROID_POSITION, {
                method: 'GET',
                headers: {
                    'Cache-Control': 'no-cache, no-store, must-revalidate',
                    'Pragma': 'no-cache',
                    'X-Android-Client': 'true',
                    'X-Android-Version': playerState.androidVersion
                }
            });
            
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}`);
            }
            
            const positionData = await response.json();
            logger.log(`Android: Server position ${positionData.position_seconds}s + ${positionData.position_milliseconds}ms`, 'ANDROID');
            
            return positionData;
        } catch (error) {
            logger.error(`Android position fetch error: ${error.message}`, 'ANDROID');
            throw error;
        }
    }
    
    async checkSyncDrift(clientPosition, clientTimestamp) {
        try {
            const response = await fetch(`${API_ENDPOINTS.SYNC_CHECK}?client_position=${clientPosition}&client_timestamp=${clientTimestamp}`);
            
            if (response.ok) {
                const data = await response.json();
                return data;
            }
        } catch (error) {
            logger.error(`Sync check failed: ${error.message}`, 'SYNC');
        }
        return null;
    }
    
    buildStreamUrl(position, timestamp) {
        let streamUrl = `${API_ENDPOINTS.DIRECT_STREAM}?t=${timestamp}&position=${position}`;
        
        // Add platform identification
        streamUrl += `&platform=${playerState.platformString}`;
        
        // iOS-specific optimizations
        if (playerState.isIOS) {
            streamUrl += `&ios_optimized=true`;
            streamUrl += `&chunk_size=32768`;        // 32KB chunks for iOS
            streamUrl += `&initial_buffer=65536`;    // 64KB initial buffer
            streamUrl += `&min_buffer_time=2`;       // 2 seconds minimum buffer
            streamUrl += `&preload=auto`;            // Force auto preload for iOS
        }
        
        return streamUrl;
    }
    
    // Calculate reconnection delay with network awareness
    calculateReconnectionDelay(attempt) {
        const baseDelay = CONFIG.RECONNECT_MIN_DELAY;
        const maxDelay = CONFIG.RECONNECT_MAX_DELAY;
        
        // Exponential backoff with jitter
        let delay = Math.min(baseDelay * Math.pow(1.5, attempt - 1), maxDelay);
        
        // Network-based adjustments
        if (playerState.networkType === '2g' || playerState.networkType === 'slow-2g') {
            delay *= NETWORK_CONFIG.SLOW_2G_MULTIPLIER;
        } else if (playerState.networkType === '3g') {
            delay *= NETWORK_CONFIG.SLOW_3G_MULTIPLIER;
        }
        
        // Battery consideration
        if (playerState.lowPowerMode) {
            delay *= 1.5;
        }
        
        // Add jitter (±25%)
        const jitter = delay * 0.25 * (Math.random() - 0.5);
        delay += jitter;
        
        return Math.max(1000, Math.round(delay)); // Minimum 1 second
    }
    
    // Battery management
    detectBatteryStatus() {
        if ('getBattery' in navigator) {
            navigator.getBattery().then(battery => {
                playerState.lowPowerMode = battery.level < 0.2 || battery.dischargingTime < 3600;
                
                if (playerState.lowPowerMode) {
                    logger.log('Low battery detected, enabling power saving mode', 'BATTERY');
                    this.emit('lowPowerMode', true);
                }
                
                // Listen for battery changes
                battery.addEventListener('levelchange', () => {
                    const wasLowPower = playerState.lowPowerMode;
                    playerState.lowPowerMode = battery.level < 0.2;
                    
                    if (wasLowPower !== playerState.lowPowerMode) {
                        logger.log(`Battery mode changed: ${playerState.lowPowerMode ? 'Low' : 'Normal'} power`, 'BATTERY');
                        this.emit('lowPowerMode', playerState.lowPowerMode);
                    }
                });
            }).catch(error => {
                logger.log(`Battery API not available: ${error.message}`, 'BATTERY');
            });
        }
    }
    
    // Event emitter
    emit(eventName, data) {
        const event = new CustomEvent(`networkManager:${eventName}`, { detail: data });
        document.dispatchEvent(event);
    }
}

// Export singleton instance
export const networkManager = new NetworkManager();