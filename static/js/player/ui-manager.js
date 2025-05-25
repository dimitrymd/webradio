// static/js/player/ui-manager.js - UI management and updates

import { CONNECTION_STATES } from './config.js';
import { playerState } from './state.js';
import { logger } from './logger.js';

export class UIManager {
    constructor() {
        this.elements = {};
        this.initializeElements();
        this.setupEventListeners();
        this.setupMediaSession();
    }
    
    initializeElements() {
        // Get all required UI elements
        const elementIds = [
            'start-btn', 'mute-btn', 'volume', 'status-message',
            'listener-count', 'current-title', 'current-artist',
            'current-album', 'current-position', 'current-duration', 
            'progress-bar'
        ];
        
        elementIds.forEach(id => {
            this.elements[id] = document.getElementById(id);
            if (!this.elements[id]) {
                logger.error(`Required UI element not found: ${id}`, 'UI');
            }
        });
        
        // Verify critical elements
        const critical = ['start-btn', 'mute-btn', 'volume', 'status-message'];
        const missing = critical.filter(id => !this.elements[id]);
        
        if (missing.length > 0) {
            throw new Error(`Critical UI elements missing: ${missing.join(', ')}`);
        }
        
        logger.log('UI elements initialized successfully', 'UI');
    }
    
    setupEventListeners() {
        // Start/Stop button
        this.elements['start-btn'].addEventListener('click', (e) => {
            e.preventDefault();
            playerState.userHasInteracted = true;
            this.emit('toggleConnection');
        });
        
        // Mute button
        this.elements['mute-btn'].addEventListener('click', (e) => {
            e.preventDefault();
            playerState.userHasInteracted = true;
            this.toggleMute();
        });
        
        // Volume control
        this.elements['volume'].addEventListener('input', (e) => {
            playerState.userHasInteracted = true;
            this.updateVolume(parseFloat(e.target.value));
        });
        
        // iOS audio unlock events
        if (playerState.isIOS) {
            const unlockEvents = ['touchstart', 'touchend', 'click', 'keydown'];
            unlockEvents.forEach(eventType => {
                document.addEventListener(eventType, this.unlockIOSAudio.bind(this), { once: true, passive: true });
            });
        }
        
        logger.log('UI event listeners setup complete', 'UI');
    }
    
    setupMediaSession() {
        if (!('mediaSession' in navigator)) {
            logger.log('Media Session API not supported', 'MEDIA');
            return;
        }
        
        try {
            // Set up action handlers
            navigator.mediaSession.setActionHandler('play', () => {
                if (!playerState.isPlaying) {
                    this.emit('requestPlay');
                }
            });
            
            navigator.mediaSession.setActionHandler('pause', () => {
                if (playerState.isPlaying) {
                    this.emit('requestStop');
                }
            });
            
            navigator.mediaSession.setActionHandler('stop', () => {
                if (playerState.isPlaying) {
                    this.emit('requestStop');
                }
            });
            
            logger.log('Media Session API configured', 'MEDIA');
        } catch (e) {
            logger.error(`Media Session setup failed: ${e.message}`, 'MEDIA');
        }
    }
    
    updateConnectionState(state) {
        const btn = this.elements['start-btn'];
        if (!btn) return;
        
        switch (state) {
            case CONNECTION_STATES.CONNECTING:
                btn.disabled = true;
                btn.textContent = 'Connecting...';
                btn.dataset.connected = 'false';
                break;
            case CONNECTION_STATES.CONNECTED:
                btn.disabled = false;
                btn.textContent = 'Disconnect';
                btn.dataset.connected = 'true';
                break;
            case CONNECTION_STATES.DISCONNECTED:
                btn.disabled = false;
                btn.textContent = 'Connect';
                btn.dataset.connected = 'false';
                break;
            case CONNECTION_STATES.RECONNECTING:
                btn.disabled = true;
                btn.textContent = 'Reconnecting...';
                break;
            case CONNECTION_STATES.ERROR:
                btn.disabled = false;
                btn.textContent = 'Retry';
                btn.dataset.connected = 'false';
                break;
        }
        
        logger.log(`UI updated for connection state: ${state}`, 'UI');
    }
    
    updateTrackInfo(trackInfo) {
        if (this.elements['current-title']) {
            this.elements['current-title'].textContent = trackInfo.title || 'Unknown Title';
        }
        
        if (this.elements['current-artist']) {
            this.elements['current-artist'].textContent = trackInfo.artist || 'Unknown Artist';
        }
        
        if (this.elements['current-album']) {
            this.elements['current-album'].textContent = trackInfo.album || 'Unknown Album';
        }
        
        if (this.elements['current-duration'] && trackInfo.duration) {
            this.elements['current-duration'].textContent = this.formatTime(trackInfo.duration);
        }
        
        // Update page title
        document.title = `${trackInfo.title || 'ChillOut Radio'} - ${trackInfo.artist || 'Unknown Artist'} | ChillOut Radio`;
        
        // Update media session
        this.updateMediaSession(trackInfo);
        
        logger.log(`UI updated for track: ${trackInfo.title}`, 'UI');
    }
    
    updateProgressBar(position, duration) {
        if (!this.elements['progress-bar'] || !duration || duration <= 0) return;
        
        const percent = Math.min((position / duration) * 100, 100);
        this.elements['progress-bar'].style.width = `${percent}%`;
        
        if (this.elements['current-position']) {
            this.elements['current-position'].textContent = this.formatTime(position);
        }
    }
    
    updateListenerCount(count) {
        if (this.elements['listener-count']) {
            this.elements['listener-count'].textContent = `Listeners: ${count}`;
        }
    }
    
    showStatus(message, isError = false, autoHide = true) {
        const statusEl = this.elements['status-message'];
        if (!statusEl) return;
        
        if (logger.debugMode || isError) {
            logger.log(`Status: ${message}`, 'UI', isError);
        }
        
        statusEl.textContent = message;
        statusEl.style.display = 'block';
        statusEl.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
        
        // Platform-specific styling
        if (playerState.isAndroid && message.includes('Android:')) {
            statusEl.style.borderLeftColor = '#4CAF50';
        }
        
        if (!isError && autoHide) {
            setTimeout(() => {
                if (statusEl.textContent === message) {
                    statusEl.style.display = 'none';
                }
            }, playerState.isPlatformMobile ? 4000 : 3000);
        }
    }
    
    hideStatus() {
        const statusEl = this.elements['status-message'];
        if (statusEl) {
            statusEl.style.display = 'none';
        }
    }
    
    toggleMute() {
        playerState.isMuted = !playerState.isMuted;
        
        // Update audio element if it exists
        if (playerState.audioElement && !playerState.isCleaningUp) {
            playerState.audioElement.muted = playerState.isMuted;
        }
        
        // Update UI
        const muteBtn = this.elements['mute-btn'];
        if (muteBtn) {
            muteBtn.textContent = playerState.isMuted ? 'Unmute' : 'Mute';
        }
        
        // Save to storage
        try {
            localStorage.setItem('radioMuted', playerState.isMuted.toString());
        } catch (e) {
            logger.error(`Failed to save mute state: ${e.message}`, 'STORAGE');
        }
        
        this.emit('muteToggled', playerState.isMuted);
    }
    
    updateVolume(volume) {
        playerState.volume = volume;
        
        // Update audio element if it exists
        if (playerState.audioElement && !playerState.isCleaningUp) {
            playerState.audioElement.volume = volume;
        }
        
        // Update UI
        const volumeEl = this.elements['volume'];
        if (volumeEl) {
            volumeEl.value = volume;
        }
        
        // Save to storage
        try {
            localStorage.setItem('radioVolume', volume.toString());
        } catch (e) {
            logger.error(`Failed to save volume: ${e.message}`, 'STORAGE');
        }
        
        this.emit('volumeChanged', volume);
    }
    
    loadSavedSettings() {
        try {
            // Load volume
            const savedVolume = localStorage.getItem('radioVolume');
            if (savedVolume !== null) {
                const volume = parseFloat(savedVolume);
                this.updateVolume(volume);
            }
            
            // Load mute state
            const savedMuted = localStorage.getItem('radioMuted');
            if (savedMuted !== null) {
                playerState.isMuted = savedMuted === 'true';
                const muteBtn = this.elements['mute-btn'];
                if (muteBtn) {
                    muteBtn.textContent = playerState.isMuted ? 'Unmute' : 'Mute';
                }
            }
            
            logger.log('UI settings loaded from storage', 'UI');
        } catch (e) {
            logger.error(`Error loading UI settings: ${e.message}`, 'STORAGE');
        }
    }
    
    unlockIOSAudio(event) {
        if (playerState.iosPlaybackUnlocked) return;
        
        logger.log("Attempting to unlock iOS audio", 'IOS');
        
        const tempAudio = new Audio();
        tempAudio.src = 'data:audio/mpeg;base64,SUQzBAAAAAAAI1RTU0UAAAAPAAADTGF2ZjU4Ljc2LjEwMAAAAAAAAAAAAAAA//OEAAAAAAAAAAAAAAAAAAAAAAAASW5mbwAAAA8AAAAEAAABIADAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMD/////////////////////wAAABhMYXZjNTguMTM=';
        
        const playPromise = tempAudio.play();
        if (playPromise !== undefined) {
            playPromise.then(() => {
                logger.log("iOS audio unlocked successfully", 'IOS');
                playerState.iosPlaybackUnlocked = true;
                tempAudio.pause();
                tempAudio.src = '';
                
                this.emit('iosAudioUnlocked');
            }).catch(err => {
                logger.error(`iOS audio unlock failed: ${err.message}`, 'IOS');
            });
        }
    }
    
    updateMediaSession(trackInfo) {
        if (!('mediaSession' in navigator) || !trackInfo) return;
        
        try {
            navigator.mediaSession.metadata = new MediaMetadata({
                title: trackInfo.title || 'ChillOut Radio',
                artist: trackInfo.artist || 'Unknown Artist',
                album: trackInfo.album || 'Live Stream',
                artwork: [
                    { src: '/static/icon-96.png', sizes: '96x96', type: 'image/png' },
                    { src: '/static/icon-192.png', sizes: '192x192', type: 'image/png' },
                    { src: '/static/icon-512.png', sizes: '512x512', type: 'image/png' }
                ]
            });
            
            if (trackInfo.duration) {
                navigator.mediaSession.setPositionState({
                    duration: trackInfo.duration,
                    playbackRate: 1.0,
                    position: playerState.getCurrentEstimatedPosition()
                });
            }
            
            logger.log(`Media session updated: ${trackInfo.title}`, 'MEDIA');
        } catch (e) {
            logger.error(`Media session update failed: ${e.message}`, 'MEDIA');
        }
    }
    
    // Utility methods
    formatTime(seconds) {
        if (!seconds || seconds < 0) return '0:00';
        const minutes = Math.floor(seconds / 60);
        const secs = Math.floor(seconds % 60);
        return `${minutes}:${secs.toString().padStart(2, '0')}`;
    }
    
    // Event emitter
    emit(eventName, data) {
        const event = new CustomEvent(`uiManager:${eventName}`, { detail: data });
        document.dispatchEvent(event);
    }
    
    // Cleanup
    cleanup() {
        // Remove any dynamic event listeners if needed
        logger.log('UI Manager cleanup completed', 'UI');
    }
}

// Export singleton instance
export const uiManager = new UIManager();