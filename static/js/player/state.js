// static/js/player/state.js - Player state management

import { PLATFORM, CONNECTION_STATES } from './config.js';

export class PlayerState {
    constructor() {
        this.reset();
    }
    
    reset() {
        // Audio and connection
        this.audioElement = null;
        this.cleanupTimeout = null;
        this.isCleaningUp = false;
        this.userHasInteracted = false;
        this.connectionId = null;
        this.lastHeartbeat = 0;
        this.connectionState = CONNECTION_STATES.DISCONNECTED;
        
        // Connection status
        this.isPlaying = false;
        this.isMuted = false;
        this.volume = 0.7;
        this.lastTrackInfoTime = Date.now();
        this.lastErrorTime = 0;
        this.reconnectAttempts = 0;
        this.isReconnecting = false;
        this.consecutiveErrors = 0;
        
        // Track info and position
        this.currentTrackId = null;
        this.currentTrack = null;
        this.serverPosition = 0;
        this.serverPositionMs = 0;
        this.trackChangeDetected = false;
        this.trackChangeTime = 0;
        
        // Position tracking
        this.lastKnownPosition = 0;
        this.positionSyncTime = 0;
        this.disconnectionTime = null;
        this.maxReconnectGap = 15000;
        this.lastPositionSave = 0;
        this.positionDriftCorrection = 0;
        this.clientStartTime = null;
        this.clientPositionOffset = 0;
        
        // Timers
        this.nowPlayingTimer = null;
        this.connectionHealthTimer = null;
        this.positionSaveTimer = null;
        this.heartbeatTimer = null;
        
        // Platform detection (static)
        this.isIOS = PLATFORM.isIOS;
        this.isSafari = PLATFORM.isSafari;
        this.isMobile = PLATFORM.isMobile;
        this.isAndroid = PLATFORM.isAndroid;
        this.androidVersion = PLATFORM.androidVersion;
        
        // Mobile-specific state
        this.backgroundTime = 0;
        this.networkType = 'unknown';
        this.lowPowerMode = false;
        
        // iOS-specific
        this.iosPlaybackUnlocked = false;
        this.pendingPlay = false;
        
        // Android-specific
        this.androidOptimized = false;
    }
    
    // Getters for computed properties
    get isPlatformMobile() {
        return this.isMobile || this.isIOS || this.isAndroid;
    }
    
    get platformString() {
        if (this.isAndroid) return 'android';
        if (this.isIOS) return 'ios';
        if (this.isMobile) return 'mobile';
        if (this.isSafari) return 'safari';
        return 'desktop';
    }
    
    get isConnected() {
        return this.connectionState === CONNECTION_STATES.CONNECTED;
    }
    
    get canReconnect() {
        return this.isPlaying && !this.isReconnecting;
    }
    
    // State management methods
    setConnectionState(newState) {
        if (this.connectionState !== newState) {
            const oldState = this.connectionState;
            this.connectionState = newState;
            this.emit('connectionStateChanged', { from: oldState, to: newState });
        }
    }
    
    setTrack(trackInfo) {
        const wasNewTrack = this.currentTrackId !== trackInfo.path;
        
        if (wasNewTrack) {
            this.currentTrackId = trackInfo.path;
            this.trackChangeDetected = true;
            this.trackChangeTime = Date.now();
            
            // Reset position tracking for new track
            this.serverPosition = 0;
            this.serverPositionMs = 0;
            this.clientStartTime = Date.now();
            this.clientPositionOffset = 0;
            this.positionDriftCorrection = 0;
            
            this.emit('trackChanged', trackInfo);
        } else {
            this.trackChangeDetected = false;
        }
        
        this.currentTrack = trackInfo;
        this.emit('trackUpdated', trackInfo);
    }
    
    updatePosition(serverPosition, serverPositionMs = 0) {
        this.serverPosition = serverPosition;
        this.serverPositionMs = serverPositionMs;
        this.lastKnownPosition = serverPosition;
        this.lastTrackInfoTime = Date.now();
        this.disconnectionTime = null;
        
        this.emit('positionUpdated', { position: serverPosition, ms: serverPositionMs });
    }
    
    recordDisconnection() {
        this.disconnectionTime = Date.now();
        this.lastKnownPosition = this.getCurrentEstimatedPosition();
        this.emit('disconnected', { position: this.lastKnownPosition, time: this.disconnectionTime });
    }
    
    getCurrentEstimatedPosition() {
        if (!this.clientStartTime) {
            return this.serverPosition;
        }
        
        const clientElapsed = (Date.now() - this.clientStartTime) / 1000;
        let estimatedPosition = this.clientPositionOffset + clientElapsed;
        
        // Apply drift correction
        if (this.positionDriftCorrection !== 0) {
            estimatedPosition += this.positionDriftCorrection;
        }
        
        // Bound by track duration
        if (this.currentTrack && this.currentTrack.duration) {
            estimatedPosition = Math.min(estimatedPosition, this.currentTrack.duration);
        }
        
        return Math.max(0, estimatedPosition);
    }
    
    calculatePositionDrift(serverPosition, clientEstimate) {
        const drift = serverPosition - clientEstimate;
        const absDrift = Math.abs(drift);
        
        // Mobile devices get more lenient drift tolerance
        const tolerance = this.isPlatformMobile ? 6 : 3;
        
        if (absDrift > tolerance) {
            // Gentler correction for mobile devices
            const correctionFactor = this.isPlatformMobile ? 0.06 : 0.1;
            this.positionDriftCorrection += drift * correctionFactor;
            
            this.emit('positionDrift', { drift, serverPosition, clientEstimate });
            return true;
        }
        
        return false;
    }
    
    incrementConsecutiveErrors() {
        this.consecutiveErrors++;
        this.lastErrorTime = Date.now();
        this.emit('errorIncremented', { count: this.consecutiveErrors });
    }
    
    resetConsecutiveErrors() {
        if (this.consecutiveErrors > 0) {
            this.consecutiveErrors = 0;
            this.emit('errorsReset');
        }
    }
    
    // Simple event emitter pattern
    emit(eventName, data) {
        const event = new CustomEvent(`playerState:${eventName}`, { detail: data });
        document.dispatchEvent(event);
    }
    
    // Save/Load state to localStorage
    saveToStorage() {
        try {
            if (this.currentTrackId && this.isPlaying) {
                const currentPos = this.getCurrentEstimatedPosition();
                const positionData = {
                    trackId: this.currentTrackId,
                    position: currentPos,
                    timestamp: Date.now(),
                    platform: this.platformString,
                    connectionId: this.connectionId
                };
                localStorage.setItem('radioPosition', JSON.stringify(positionData));
                
                // Save other settings
                localStorage.setItem('radioVolume', this.volume.toString());
                localStorage.setItem('radioMuted', this.isMuted.toString());
                
                this.lastPositionSave = Date.now();
            }
        } catch (e) {
            // Ignore storage errors
            console.warn('Failed to save state to storage:', e);
        }
    }
    
    loadFromStorage() {
        try {
            // Load position
            const saved = localStorage.getItem('radioPosition');
            if (saved) {
                const data = JSON.parse(saved);
                const age = Date.now() - data.timestamp;
                const maxAge = this.isPlatformMobile ? 45000 : 30000;
                
                if (age < maxAge) {
                    this.lastKnownPosition = data.position + Math.floor(age / 1000);
                    return data;
                }
            }
            
            // Load settings
            const savedVolume = localStorage.getItem('radioVolume');
            if (savedVolume !== null) {
                this.volume = parseFloat(savedVolume);
            }
            
            const savedMuted = localStorage.getItem('radioMuted');
            if (savedMuted !== null) {
                this.isMuted = savedMuted === 'true';
            }
            
        } catch (e) {
            console.warn('Failed to load state from storage:', e);
        }
        return null;
    }
    
    // Cleanup method
    cleanup() {
        // Clear all timers
        const timers = [
            'nowPlayingTimer',
            'connectionHealthTimer', 
            'positionSaveTimer',
            'heartbeatTimer'
        ];
        
        timers.forEach(timer => {
            if (this[timer]) {
                clearInterval(this[timer]);
                this[timer] = null;
            }
        });
        
        if (this.cleanupTimeout) {
            clearTimeout(this.cleanupTimeout);
            this.cleanupTimeout = null;
        }
        
        // Save final state
        this.saveToStorage();
    }
}

// Export singleton instance
export const playerState = new PlayerState();