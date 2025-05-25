// static/js/player/main-player.js - Main player controller that orchestrates all modules

import { CONFIG, CONNECTION_STATES } from './config.js';
import { playerState } from './state.js';
import { audioManager } from './audio-manager.js';
import { networkManager } from './network-manager.js';
import { uiManager } from './ui-manager.js';
import { logger } from './logger.js';

export class MainPlayer {
    constructor() {
        this.timers = new Map();
        this.setupEventListeners();
        this.setupVisibilityHandling();
    }
    
    async initialize() {
        try {
            logger.log("Initializing ChillOut Radio player with modular architecture", 'INIT');
            
            const platformType = playerState.isPlatformMobile ? 'Mobile' : 'Desktop';
            const platformDetails = playerState.platformString;
            const versionInfo = playerState.isAndroid ? `v${playerState.androidVersion}` : 'N/A';
            
            logger.log(`Platform: ${platformType}, ${platformDetails} (${versionInfo})`, 'INIT');
            
            // Load saved settings
            uiManager.loadSavedSettings();
            playerState.loadFromStorage();
            
            // Set up mobile-specific features
            if (playerState.isPlatformMobile) {
                this.setupMobileFeatures();
            }
            
            // Initialize network monitoring
            networkManager.detectBatteryStatus();
            
            // Fetch initial track info
            await this.fetchNowPlaying();
            
            // Set up periodic timers
            this.setupTimers();
            
            // Initialize connection state
            uiManager.updateConnectionState(CONNECTION_STATES.DISCONNECTED);
            
            logger.log('Player initialization completed successfully', 'INIT');
            uiManager.showStatus('Player ready - tap Connect to start streaming', false, false);
            
        } catch (error) {
            logger.error(`Player initialization failed: ${error.message}`, 'INIT');
            uiManager.showStatus(`Initialization failed: ${error.message}`, true);
            throw error;
        }
    }
    
    setupEventListeners() {
        // UI Manager events
        document.addEventListener('uiManager:toggleConnection', () => {
            this.toggleConnection();
        });
        
        document.addEventListener('uiManager:requestPlay', () => {
            if (!playerState.isPlaying) {
                this.startAudio();
            }
        });
        
        document.addEventListener('uiManager:requestStop', () => {
            if (playerState.isPlaying) {
                this.stopAudio();
            }
        });
        
        // iOS Manager events
        document.addEventListener('iosManager:needsReconnection', (e) => {
            this.attemptReconnection(e.detail);
        });
        
        document.addEventListener('iosManager:sendHeartbeat', () => {
            networkManager.sendHeartbeat();
        });
        
        document.addEventListener('iosManager:pauseTimers', () => {
            this.pauseAllTimers();
        });
        
        document.addEventListener('iosManager:resumeTimers', () => {
            this.resumeAllTimers();
        });
        
        document.addEventListener('iosManager:cleanupAudio', async () => {
            // iOS-specific reconnection handling
        if (playerState.isIOS && reason.includes('iOS')) {
            try {
                await iosManager.handleIOSReconnection();
                setTimeout(() => {
                    playerState.isReconnecting = false;
                }, 2000);
                return;
            } catch (error) {
                logger.error(`iOS reconnection failed: ${error.message}`, 'IOS');
                // Fall through to standard reconnection
            }
        }
        
        await audioManager.cleanupAudioElement();
        });
        
        document.addEventListener('iosManager:createAudio', () => {
            audioManager.createAudioElement();
        });
        
        document.addEventListener('iosManager:requiresInteraction', () => {
            this.handleUserInteractionRequired();
        });
        
        document.addEventListener('iosManager:fetchPosition', async () => {
            await this.fetchNowPlaying();
        });
        
        document.addEventListener('iosManager:startStreaming', async () => {
            await this.startStreaming();
        });
        
        document.addEventListener('iosManager:audioUnlocked', () => {
            if (playerState.pendingPlay) {
                playerState.pendingPlay = false;
                this.attemptPlayback();
            }
        });
        
        // iOS Streaming Manager events
        document.addEventListener('iosStreaming:forceReconnection', (e) => {
            logger.log(`iOS streaming force reconnection: ${e.detail}`, 'IOS_STREAM');
            this.attemptReconnection(e.detail);
        });
        
        // Audio Manager events
        document.addEventListener('audioManager:playing', () => {
            uiManager.showStatus('Stream connected and playing');
        });
        
        document.addEventListener('audioManager:waiting', () => {
            uiManager.showStatus('Buffering...', false, false);
        });
        
        document.addEventListener('audioManager:stalled', () => {
            uiManager.showStatus('Stream stalled - buffering', true, false);
        });
        
        document.addEventListener('audioManager:ended', () => {
            uiManager.showStatus('Track ended - getting next track', false, false);
            this.attemptReconnection('track ended');
        });
        
        document.addEventListener('audioManager:needsReconnection', (e) => {
            this.attemptReconnection(e.detail);
        });
        
        document.addEventListener('audioManager:timeUpdate', (e) => {
            const { position, duration } = e.detail;
            uiManager.updateProgressBar(position, duration);
        });
        
        // Network Manager events
        document.addEventListener('networkManager:networkChanged', (e) => {
            const { from, to } = e.detail;
            uiManager.showStatus(`Network: ${to.toUpperCase()}`, false, true);
        });
        
        document.addEventListener('networkManager:networkRestored', () => {
            if (playerState.isPlaying && playerState.audioElement && playerState.audioElement.paused) {
                uiManager.showStatus('Connection restored - reconnecting...', false, true);
                setTimeout(() => {
                    this.attemptReconnection('network restored');
                }, 1000);
            }
        });
        
        document.addEventListener('networkManager:networkLost', () => {
            uiManager.showStatus('Network connection lost', true);
        });
        
        document.addEventListener('networkManager:listenerCountUpdated', (e) => {
            uiManager.updateListenerCount(e.detail);
        });
        
        document.addEventListener('networkManager:lowPowerMode', (e) => {
            if (e.detail) {
                uiManager.showStatus('Low battery - enabled power saving mode', false, true);
                this.adaptTimersForPowerSaving();
            }
        });
        
        // Player State events
        document.addEventListener('playerState:connectionStateChanged', (e) => {
            const { to } = e.detail;
            uiManager.updateConnectionState(to);
        });
        
        document.addEventListener('playerState:trackChanged', (e) => {
            logger.log(`Track changed: ${e.detail.title}`, 'TRACK');
            
            if (playerState.isPlaying && playerState.audioElement && !playerState.isReconnecting) {
                logger.log("Track changed while playing, will reconnect after grace period", 'TRACK');
                
                setTimeout(() => {
                    if (playerState.isPlaying && playerState.trackChangeDetected && !playerState.isReconnecting) {
                        logger.log("Grace period ended, reconnecting for new track", 'TRACK');
                        this.attemptReconnection('track change');
                    }
                }, CONFIG.TRACK_CHANGE_GRACE_PERIOD);
            }
        });
        
        document.addEventListener('playerState:trackUpdated', (e) => {
            uiManager.updateTrackInfo(e.detail);
        });
        
        document.addEventListener('playerState:disconnected', () => {
            logger.log(`Recorded disconnection at position ${playerState.lastKnownPosition.toFixed(1)}s`, 'CONNECTION');
        });
    }
    
    setupVisibilityHandling() {
        document.addEventListener('visibilitychange', () => {
            if (document.hidden) {
                playerState.backgroundTime = Date.now();
                logger.log('App went to background', 'VISIBILITY');
                
                if (playerState.isPlaying) {
                    playerState.saveToStorage();
                    this.adaptTimersForBackground();
                }
            } else {
                if (playerState.backgroundTime > 0) {
                    const backgroundDuration = Date.now() - playerState.backgroundTime;
                    logger.log(`App returned to foreground after ${Math.round(backgroundDuration/1000)}s`, 'VISIBILITY');
                    
                    if (playerState.isPlaying) {
                        this.restoreNormalTimers();
                        networkManager.sendHeartbeat();
                        
                        // Check if audio is still playing after background
                        setTimeout(() => {
                            if (playerState.audioElement && playerState.audioElement.paused && playerState.isPlaying) {
                                logger.log('Audio paused during background, attempting recovery', 'VISIBILITY');
                                this.attemptReconnection('background recovery');
                            } else {
                                this.fetchNowPlaying();
                            }
                        }, 1000);
                    }
                }
            }
        });
    }
    
    setupMobileFeatures() {
        logger.log('Setting up mobile-specific features', 'MOBILE');
        
        // Android-specific optimizations
        if (playerState.isAndroid) {
            playerState.androidOptimized = true;
            logger.log(`Android optimizations enabled for version ${playerState.androidVersion}`, 'ANDROID');
        }
        
        // Service worker registration
        if ('serviceWorker' in navigator) {
            navigator.serviceWorker.register('/sw.js').then(registration => {
                logger.log('Service Worker registered successfully', 'SW');
            }).catch(error => {
                logger.log(`Service Worker registration failed: ${error.message}`, 'SW');
            });
        }
    }
    
    setupTimers() {
        // Now playing timer
        const nowPlayingInterval = playerState.isPlatformMobile ? 
            CONFIG.NOW_PLAYING_INTERVAL : 8000;
        
        this.timers.set('nowPlaying', setInterval(() => {
            this.fetchNowPlaying();
        }, nowPlayingInterval));
        
        // Connection health timer
        const healthInterval = playerState.isPlatformMobile ? 
            CONFIG.CONNECTION_CHECK_INTERVAL : 5000;
        
        this.timers.set('connectionHealth', setInterval(() => {
            this.checkConnectionHealth();
        }, healthInterval));
        
        // Position save timer
        this.timers.set('positionSave', setInterval(() => {
            playerState.saveToStorage();
        }, CONFIG.POSITION_SAVE_INTERVAL));
        
        // Heartbeat timer
        this.timers.set('heartbeat', setInterval(() => {
            networkManager.sendHeartbeat();
        }, CONFIG.MOBILE_HEARTBEAT_INTERVAL));
        
        logger.log(`Timers configured: nowPlaying=${nowPlayingInterval}ms, health=${healthInterval}ms`, 'CONTROL');
    }
    
    adaptTimersForBackground() {
        // Reduce timer frequency when in background to save battery
        this.clearTimer('nowPlaying');
        this.timers.set('nowPlaying', setInterval(() => {
            this.fetchNowPlaying();
        }, 30000)); // 30 seconds in background
        
        logger.log('Timers adapted for background mode', 'VISIBILITY');
    }
    
    adaptTimersForPowerSaving() {
        // Extend intervals to save battery
        Object.keys(CONFIG).forEach(key => {
            if (key.includes('INTERVAL')) {
                CONFIG[key] *= 1.5;
            }
        });
        
        logger.log('Timers adapted for power saving mode', 'BATTERY');
    }
    
    restoreNormalTimers() {
        // Restore normal timer frequencies
        this.clearTimer('nowPlaying');
        const nowPlayingInterval = playerState.isPlatformMobile ? 
            CONFIG.NOW_PLAYING_INTERVAL : 8000;
        
        this.timers.set('nowPlaying', setInterval(() => {
            this.fetchNowPlaying();
        }, nowPlayingInterval));
        
        logger.log('Timers restored to normal frequency', 'VISIBILITY');
    }
    
    async startAudio() {
        logger.log('Starting audio playback', 'CONTROL');
        
        if (playerState.isPlaying || playerState.isReconnecting) {
            logger.log('Already playing or reconnecting, ignoring start request', 'CONTROL');
            return;
        }
        
        // Update connection state
        playerState.setConnectionState(CONNECTION_STATES.CONNECTING);
        uiManager.showStatus('Connecting to stream...', false, false);
        
        // Reset state
        playerState.isPlaying = true;
        playerState.isReconnecting = false;
        playerState.reconnectAttempts = 0;
        playerState.trackChangeDetected = false;
        playerState.pendingPlay = false;
        playerState.positionDriftCorrection = 0;
        playerState.consecutiveErrors = 0;
        
        try {
            // Get fresh track info and position
            await this.fetchNowPlaying();
            
            logger.log(`Starting playback with server position: ${playerState.serverPosition}s + ${playerState.serverPositionMs}ms`, 'CONTROL');
            
            // Initialize client position tracking
            playerState.clientStartTime = Date.now();
            playerState.clientPositionOffset = playerState.serverPosition;
            
            // Clean up and create new audio element
            await audioManager.cleanupAudioElement();
            audioManager.createAudioElement();
            
            await this.startStreaming();
            
        } catch (error) {
            logger.error(`Failed to start audio: ${error.message}`, 'CONTROL');
            
            // Try with saved position as fallback
            const savedPos = playerState.loadFromStorage();
            if (savedPos && savedPos.trackId === playerState.currentTrackId) {
                playerState.serverPosition = savedPos.position;
                logger.log(`Using saved position: ${playerState.serverPosition}s`, 'CONTROL');
            } else {
                playerState.serverPosition = 0;
                logger.log('No reliable position data, starting from beginning', 'CONTROL');
            }
            
            playerState.clientStartTime = Date.now();
            playerState.clientPositionOffset = playerState.serverPosition;
            
            try {
                await audioManager.cleanupAudioElement();
                audioManager.createAudioElement();
                await this.startStreaming();
            } catch (fallbackError) {
                logger.error(`Fallback start failed: ${fallbackError.message}`, 'CONTROL');
                this.stopAudio(true);
            }
        }
    }
    
    async startStreaming() {
        const timestamp = Date.now();
        const syncPosition = playerState.serverPosition;
        
        // Build stream URL
        const streamUrl = networkManager.buildStreamUrl(syncPosition, timestamp);
        
        logger.log(`Starting streaming with URL: ${streamUrl}`, 'CONTROL');
        
        // Update client position tracking
        playerState.clientStartTime = Date.now();
        playerState.clientPositionOffset = syncPosition;
        playerState.disconnectionTime = null;
        
        try {
            await audioManager.attemptPlayback(streamUrl);
            logger.log('Streaming started successfully', 'CONTROL');
        } catch (error) {
            if (error.name === 'NotAllowedError') {
                this.handleUserInteractionRequired();
            } else {
                throw error;
            }
        }
    }
    
    handleUserInteractionRequired() {
        uiManager.showStatus('Please tap to enable audio playback', true, false);
        playerState.setConnectionState(CONNECTION_STATES.ERROR);
        
        // Set up one-time click handler
        const btn = uiManager.elements['start-btn'];
        btn.onclick = () => {
            playerState.userHasInteracted = true;
            this.attemptPlayback();
        };
    }
    
    async attemptPlayback() {
        if (!playerState.audioElement || playerState.isCleaningUp) {
            logger.error('No audio element available for playback', 'AUDIO');
            return;
        }
        
        try {
            const playPromise = playerState.audioElement.play();
            if (playPromise !== undefined) {
                await playPromise;
                logger.log('Playback started successfully', 'AUDIO');
            }
        } catch (error) {
            logger.error(`Playback failed: ${error.message}`, 'AUDIO');
            throw error;
        }
    }
    
    stopAudio(isError = false) {
        logger.log(`Stopping audio playback${isError ? ' (due to error)' : ''}`, 'CONTROL');
        
        // Record disconnection for continuity
        if (isError && playerState.isPlaying) {
            playerState.recordDisconnection();
        }
        
        playerState.isPlaying = false;
        playerState.isReconnecting = false;
        playerState.pendingPlay = false;
        
        // Update connection state
        playerState.setConnectionState(CONNECTION_STATES.DISCONNECTED);
        
        // Clean up audio
        audioManager.cleanup();
        
        if (!isError) {
            uiManager.showStatus('Disconnected from audio stream');
        }
    }
    
    toggleConnection() {
        const isConnected = playerState.connectionState === CONNECTION_STATES.CONNECTED || playerState.isPlaying;
        
        if (isConnected) {
            logger.log('User requested disconnect', 'CONTROL');
            this.stopAudio();
        } else {
            logger.log('User requested connect', 'CONTROL');
            this.startAudio();
        }
    }
    
    pauseAllTimers() {
        logger.log('Pausing all timers', 'CONTROL');
        for (const [name, timer] of this.timers) {
            clearInterval(timer);
        }
    }
    
    resumeAllTimers() {
        logger.log('Resuming all timers', 'CONTROL');
        this.setupTimers(); // Recreate all timers
    }
    
    async attemptReconnection(reason = 'unknown') {
        if (playerState.isReconnecting) {
            logger.log(`Reconnection already in progress, ignoring request (reason: ${reason})`, 'CONNECTION');
            return;
        }
        
        if (!playerState.isPlaying) {
            logger.log(`Not playing, ignoring reconnection request (reason: ${reason})`, 'CONNECTION');
            return;
        }
        
        if (playerState.reconnectAttempts >= CONFIG.RECONNECT_ATTEMPTS) {
            logger.error(`Maximum reconnection attempts (${CONFIG.RECONNECT_ATTEMPTS}) reached`, 'CONNECTION');
            uiManager.showStatus('Could not reconnect to server. Please try again later.', true);
            this.stopAudio(true);
            return;
        }
        
        playerState.isReconnecting = true;
        playerState.reconnectAttempts++;
        playerState.setConnectionState(CONNECTION_STATES.RECONNECTING);
        
        const delay = networkManager.calculateReconnectionDelay(playerState.reconnectAttempts);
        
        logger.log(`Reconnection attempt ${playerState.reconnectAttempts}/${CONFIG.RECONNECT_ATTEMPTS} in ${Math.round(delay/1000)}s (reason: ${reason})`, 'CONNECTION');
        uiManager.showStatus(`Reconnecting (${playerState.reconnectAttempts}/${CONFIG.RECONNECT_ATTEMPTS})...`, true, false);
        
        await audioManager.cleanupAudioElement();
        
        setTimeout(async () => {
            if (!playerState.isPlaying) {
                playerState.isReconnecting = false;
                return;
            }
            
            logger.log(`Executing reconnection attempt ${playerState.reconnectAttempts}`, 'CONNECTION');
            
            try {
                audioManager.createAudioElement();
                await this.fetchNowPlaying();
                await this.startStreaming();
            } catch (error) {
                logger.error(`Reconnection failed: ${error.message}`, 'CONNECTION');
                // Will try again automatically if attempts remain
            }
            
            setTimeout(() => {
                playerState.isReconnecting = false;
            }, 3000);
        }, delay);
    }
    
    async fetchNowPlaying() {
        try {
            const data = await networkManager.fetchNowPlaying();
            this.updateTrackInfo(data);
        } catch (error) {
            logger.error(`Failed to fetch now playing: ${error.message}`, 'API');
            // Don't throw - this is called periodically
        }
    }
    
    updateTrackInfo(info) {
        try {
            if (info.error) {
                uiManager.showStatus(`Server error: ${info.error}`, true);
                return;
            }
            
            // Update track information
            playerState.setTrack(info);
            
            // Update position if provided
            if (info.playback_position !== undefined) {
                const serverPosition = info.playback_position;
                const serverPositionMs = info.playback_position_ms || 0;
                
                // Calculate drift and apply correction if needed
                const clientEstimate = playerState.getCurrentEstimatedPosition();
                playerState.calculatePositionDrift(serverPosition, clientEstimate);
                
                playerState.updatePosition(serverPosition, serverPositionMs);
            }
            
            // Update listener count
            if (info.active_listeners !== undefined) {
                uiManager.updateListenerCount(info.active_listeners);
            }
            
        } catch (e) {
            logger.error(`Error processing track info: ${e.message}`, 'TRACK');
        }
    }
    
    checkConnectionHealth() {
        if (!playerState.isPlaying || playerState.isReconnecting) return;
        
        const now = Date.now();
        const timeSinceLastTrackInfo = (now - playerState.lastTrackInfoTime) / 1000;
        const timeSinceLastHeartbeat = (now - playerState.lastHeartbeat) / 1000;
        
        // Check if we need fresh track info
        if (timeSinceLastTrackInfo > CONFIG.NOW_PLAYING_INTERVAL / 1000) {
            this.fetchNowPlaying();
        }
        
        // Check if heartbeat is too old
        if (timeSinceLastHeartbeat > CONFIG.MOBILE_HEARTBEAT_INTERVAL / 1000 * 2) {
            networkManager.sendHeartbeat();
        }
        
        // Check audio element health
        if (playerState.audioElement && !playerState.isCleaningUp) {
            if (playerState.audioElement.paused && playerState.isPlaying && !playerState.trackChangeDetected) {
                logger.error('Audio is paused unexpectedly', 'HEALTH');
                
                this.attemptPlayback().catch(() => {
                    this.attemptReconnection('unexpected pause');
                });
            }
            
            if (playerState.audioElement.networkState === HTMLMediaElement.NETWORK_NO_SOURCE) {
                logger.error('Audio has no source', 'HEALTH');
                this.attemptReconnection('no source');
            }
        }
    }
    
    clearTimer(name) {
        if (this.timers.has(name)) {
            clearInterval(this.timers.get(name));
            this.timers.delete(name);
        }
    }
    
    cleanup() {
        logger.log('Starting player cleanup', 'CLEANUP');
        
        // Clear all timers
        for (const [name, timer] of this.timers) {
            clearInterval(timer);
            logger.log(`Cleared timer: ${name}`, 'CLEANUP');
        }
        this.timers.clear();
        
        // Cleanup all modules
        audioManager.cleanup();
        playerState.cleanup();
        uiManager.cleanup();
        
        // iOS-specific cleanup
        if (playerState.isIOS) {
            iosManager.cleanup();
            iosStreamingManager.cleanup();
        }
        
        logger.log('Player cleanup completed', 'CLEANUP');
    }
}

// Initialize and export singleton
export const mainPlayer = new MainPlayer();

// Auto-initialize when DOM is ready
document.addEventListener('DOMContentLoaded', async () => {
    try {
        await mainPlayer.initialize();
    } catch (error) {
        logger.error(`Failed to initialize player: ${error.message}`, 'INIT');
        alert(`Player initialization failed: ${error.message}`);
    }
});

// Cleanup on page unload
window.addEventListener('beforeunload', () => {
    mainPlayer.cleanup();
});