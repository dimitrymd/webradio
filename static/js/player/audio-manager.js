// static/js/player/audio-manager.js - Audio element management

import { CONFIG, AUDIO_CONFIG, ERROR_CODES } from './config.js';
import { playerState } from './state.js';
import { logger } from './logger.js';

export class AudioManager {
    constructor() {
        this.wakeLock = null;
        this.audioContext = null;
        this.setupAudioContext();
    }
    
    setupAudioContext() {
        if (!('AudioContext' in window) && !('webkitAudioContext' in window)) {
            logger.log('AudioContext not supported', 'AUDIO');
            return;
        }
        
        try {
            const AudioContextClass = window.AudioContext || window.webkitAudioContext;
            this.audioContext = new AudioContextClass();
            
            if (this.audioContext.state === 'suspended') {
                logger.log('AudioContext suspended, will resume on user interaction', 'AUDIO');
                this.setupAudioContextResume();
            }
        } catch (e) {
            logger.error(`AudioContext creation failed: ${e.message}`, 'AUDIO');
        }
    }
    
    setupAudioContextResume() {
        const resumeAudioContext = () => {
            if (this.audioContext && this.audioContext.state === 'suspended') {
                this.audioContext.resume().then(() => {
                    logger.log('AudioContext resumed successfully', 'AUDIO');
                }).catch(e => {
                    logger.error(`AudioContext resume failed: ${e.message}`, 'AUDIO');
                });
            }
        };
        
        ['touchstart', 'touchend', 'mousedown', 'keydown'].forEach(eventType => {
            document.addEventListener(eventType, resumeAudioContext, { once: true });
        });
    }
    
    createAudioElement() {
        if (playerState.audioElement && !playerState.isCleaningUp) {
            logger.log('Audio element already exists', 'AUDIO');
            return playerState.audioElement;
        }
        
        logger.log('Creating mobile-optimized audio element', 'AUDIO');
        
        const audio = new Audio();
        audio.controls = false;
        audio.volume = playerState.volume;
        audio.muted = playerState.isMuted;
        audio.crossOrigin = AUDIO_CONFIG.CROSSORIGIN;
        
        // Platform-specific settings
        if (playerState.isPlatformMobile) {
            audio.preload = AUDIO_CONFIG.PRELOAD_MOBILE;
            audio.autoplay = AUDIO_CONFIG.MOBILE_AUTOPLAY;
            
            if (audio.setAttribute) {
                audio.setAttribute('webkit-playsinline', 'true');
                audio.setAttribute('playsinline', 'true');
            }
            
            if (playerState.isIOS) {
                audio.playsInline = AUDIO_CONFIG.IOS_PLAYSINLINE;
            }
        } else {
            audio.preload = AUDIO_CONFIG.PRELOAD_DESKTOP;
            audio.autoplay = AUDIO_CONFIG.DESKTOP_AUTOPLAY;
        }
        
        // Set up event listeners
        this.setupAudioEventListeners(audio);
        
        playerState.audioElement = audio;
        
        // Emit event for iOS streaming manager
        this.emit('audioCreated', { audioElement: audio });
        
        logger.log('Mobile-optimized audio element created', 'AUDIO');
        
        return audio;
    }
    
    setupAudioEventListeners(audio) {
        // Playing event
        audio.addEventListener('playing', () => {
            logger.log('Audio playing', 'AUDIO');
            playerState.setConnectionState('connected');
            playerState.resetConsecutiveErrors();
            playerState.trackChangeDetected = false;
            playerState.pendingPlay = false;
            
            // Reset position tracking when playback starts
            playerState.clientStartTime = Date.now();
            
            // Request wake lock for mobile
            if (playerState.isPlatformMobile) {
                this.requestWakeLock();
            }
            
            this.emit('playing');
        });
        
        // Waiting event (buffering)
        audio.addEventListener('waiting', () => {
            logger.log('Audio buffering', 'AUDIO');
            this.emit('waiting');
        });
        
        // Stalled event
        audio.addEventListener('stalled', () => {
            logger.log('Audio stalled', 'AUDIO');
            this.emit('stalled');
            
            if (!playerState.isReconnecting && !playerState.trackChangeDetected) {
                const stalledTimeout = playerState.isPlatformMobile ? 
                    CONFIG.MOBILE_BUFFER_TIMEOUT : 5000;
                    
                setTimeout(() => {
                    if (playerState.isPlaying && !playerState.isReconnecting && 
                        audio && audio.readyState < 3) {
                        logger.log('Still stalled after timeout, requesting reconnection', 'AUDIO');
                        this.emit('needsReconnection', 'stalled playback');
                    }
                }, stalledTimeout);
            }
        });
        
        // Error event
        audio.addEventListener('error', (e) => {
            const errorCode = e.target.error ? e.target.error.code : 'unknown';
            const errorMsg = this.getErrorMessage(e.target.error);
            
            playerState.incrementConsecutiveErrors();
            logger.error(`Audio error: ${errorMsg} (code ${errorCode}, consecutive: ${playerState.consecutiveErrors})`, 'AUDIO');
            
            if (playerState.isPlaying && !playerState.isCleaningUp) {
                const now = Date.now();
                
                if (now - playerState.lastErrorTime > CONFIG.MAX_ERROR_FREQUENCY) {
                    this.handleAudioError(errorCode, errorMsg);
                }
            }
        });
        
        // Ended event
        audio.addEventListener('ended', () => {
            logger.log('Audio ended', 'AUDIO');
            
            if (playerState.isPlaying && !playerState.isReconnecting) {
                if (playerState.trackChangeDetected) {
                    logger.log('Audio ended during track change, reconnecting to new track', 'AUDIO');
                } else {
                    logger.log('Audio ended unexpectedly, attempting to recover', 'AUDIO');
                }
                
                this.emit('ended');
            }
        });
        
        // Progress monitoring
        audio.addEventListener('timeupdate', () => {
            if (audio && !playerState.isCleaningUp && playerState.currentTrack) {
                const estimatedPosition = playerState.getCurrentEstimatedPosition();
                this.emit('timeUpdate', {
                    position: estimatedPosition,
                    duration: playerState.currentTrack.duration
                });
            }
        });
        
        // Mobile-specific events
        if (playerState.isPlatformMobile) {
            audio.addEventListener('loadstart', () => {
                logger.log('Mobile: Audio load started', 'MOBILE');
            });
            
            audio.addEventListener('canplay', () => {
                logger.log('Mobile: Audio can start playing', 'MOBILE');
            });
        }
    }
    
    handleAudioError(errorCode, errorMsg) {
        logger.error(`Handling audio error: code ${errorCode}, message: ${errorMsg}`, 'AUDIO');
        
        // Record position for continuity
        playerState.recordDisconnection();
        
        let reconnectDelay = CONFIG.RECONNECT_MIN_DELAY;
        
        if (playerState.consecutiveErrors > 3) {
            reconnectDelay = Math.min(CONFIG.RECONNECT_MAX_DELAY, 
                reconnectDelay * playerState.consecutiveErrors);
        } else if (errorCode === ERROR_CODES.MEDIA_ERR_SRC_NOT_SUPPORTED) {
            reconnectDelay = playerState.isPlatformMobile ? 3000 : 2000;
        } else if (errorCode === ERROR_CODES.MEDIA_ERR_NETWORK) {
            reconnectDelay = playerState.networkType === '2g' ? 5000 : 
                (playerState.isPlatformMobile ? 3000 : 2000);
        } else {
            reconnectDelay = playerState.isPlatformMobile ? 3000 : 2000;
        }
        
        setTimeout(() => {
            if (playerState.canReconnect) {
                this.emit('needsReconnection', `error code ${errorCode}`);
            }
        }, reconnectDelay);
    }
    
    async attemptPlayback(streamUrl) {
        const audio = playerState.audioElement;
        if (!audio || playerState.isCleaningUp) {
            throw new Error('No audio element available for playback');
        }
        
        logger.log(`Setting audio source: ${streamUrl}`, 'AUDIO');
        audio.src = streamUrl;
        
        // Platform-specific playback delay
        const playDelay = playerState.isPlatformMobile ? 800 : 200;
        
        return new Promise((resolve, reject) => {
            setTimeout(async () => {
                if (!audio || !playerState.isPlaying || playerState.isCleaningUp) {
                    reject(new Error('Playback cancelled'));
                    return;
                }
                
                try {
                    const playPromise = audio.play();
                    if (playPromise !== undefined) {
                        await playPromise;
                        resolve();
                    } else {
                        resolve();
                    }
                } catch (error) {
                    reject(error);
                }
            }, playDelay);
        });
    }
    
    async cleanupAudioElement() {
        return new Promise((resolve) => {
            if (playerState.cleanupTimeout) {
                clearTimeout(playerState.cleanupTimeout);
                playerState.cleanupTimeout = null;
            }
            
            if (!playerState.audioElement) {
                resolve();
                return;
            }
            
            logger.log('Cleaning up audio element', 'AUDIO');
            playerState.isCleaningUp = true;
            
            const elementToCleanup = playerState.audioElement;
            playerState.audioElement = null;
            
            // Stop playback
            try {
                elementToCleanup.pause();
            } catch (e) {
                logger.error(`Error pausing during cleanup: ${e.message}`, 'AUDIO');
            }
            
            // Clear source
            try {
                elementToCleanup.src = '';
                elementToCleanup.load();
            } catch (e) {
                logger.error(`Error clearing source during cleanup: ${e.message}`, 'AUDIO');
            }
            
            // Cleanup delay (longer for mobile)
            const cleanupDelay = playerState.isPlatformMobile ? 
                CONFIG.CLEANUP_DELAY * 2 : CONFIG.CLEANUP_DELAY;
            
            playerState.cleanupTimeout = setTimeout(() => {
                try {
                    if (elementToCleanup.parentNode) {
                        elementToCleanup.remove();
                    }
                } catch (e) {
                    logger.error(`Error removing element during cleanup: ${e.message}`, 'AUDIO');
                }
                
                playerState.isCleaningUp = false;
                playerState.cleanupTimeout = null;
                resolve();
            }, cleanupDelay);
        });
    }
    
    // Wake lock management
    async requestWakeLock() {
        if (!('wakeLock' in navigator)) {
            logger.log('Wake Lock API not supported', 'WAKE');
            return;
        }
        
        try {
            this.wakeLock = await navigator.wakeLock.request('screen');
            logger.log('Wake lock acquired', 'WAKE');
            
            this.wakeLock.addEventListener('release', () => {
                logger.log('Wake lock released', 'WAKE');
                this.wakeLock = null;
            });
            
        } catch (e) {
            logger.error(`Wake lock failed: ${e.message}`, 'WAKE');
        }
    }
    
    async releaseWakeLock() {
        if (this.wakeLock) {
            try {
                await this.wakeLock.release();
                this.wakeLock = null;
                logger.log('Wake lock released manually', 'WAKE');
            } catch (e) {
                logger.error(`Wake lock release failed: ${e.message}`, 'WAKE');
            }
        }
    }
    
    // Utility methods
    getErrorMessage(error) {
        if (!error) return 'Unknown error';
        
        switch (error.code) {
            case ERROR_CODES.MEDIA_ERR_ABORTED:
                return 'Playback aborted';
            case ERROR_CODES.MEDIA_ERR_NETWORK:
                return 'Network error';
            case ERROR_CODES.MEDIA_ERR_DECODE:
                return 'Decoding error';
            case ERROR_CODES.MEDIA_ERR_SRC_NOT_SUPPORTED:
                return 'Format not supported';
            default:
                return `Media error (code ${error.code})`;
        }
    }
    
    // Event emitter
    emit(eventName, data) {
        const event = new CustomEvent(`audioManager:${eventName}`, { detail: data });
        document.dispatchEvent(event);
    }
    
    // Cleanup
    cleanup() {
        this.releaseWakeLock();
        this.cleanupAudioElement();
    }
}

// Export singleton instance
export const audioManager = new AudioManager();