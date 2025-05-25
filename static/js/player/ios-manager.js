// static/js/player/ios-streaming-fixes.js - iOS-specific streaming and buffer management

import { CONFIG } from './config.js';
import { playerState } from './state.js';
import { logger } from './logger.js';

export class IOSStreamingManager {
    constructor() {
        this.bufferHealthTimer = null;
        this.streamQualityTimer = null;
        this.preloadManager = null;
        this.fallbackAttempts = 0;
        this.maxFallbackAttempts = 3;
        this.setupIOSStreamingOptimizations();
    }
    
    setupIOSStreamingOptimizations() {
        if (!playerState.isIOS) return;
        
        logger.log('Setting up iOS streaming optimizations', 'IOS_STREAM');
        
        // Override default configurations for iOS
        this.adjustConfigForIOS();
        
        // Setup iOS-specific buffer monitoring
        this.setupBufferHealthMonitoring();
        
        // Setup stream quality monitoring
        this.setupStreamQualityMonitoring();
        
        // Setup iOS-specific audio element optimizations
        this.setupIOSAudioOptimizations();
        
        // Setup iOS chunked loading
        this.setupIOSChunkedLoading();
    }
    
    adjustConfigForIOS() {
        // iOS Safari needs different buffer timeouts
        CONFIG.MOBILE_BUFFER_TIMEOUT = 8000;  // Shorter timeout for iOS
        CONFIG.IOS_BUFFER_RECOVERY_DELAY = 1000;  // Quick recovery
        CONFIG.IOS_STALL_RECOVERY_DELAY = 2000;   // Stall recovery
        CONFIG.IOS_MAX_BUFFER_RETRIES = 5;        // Multiple buffer retries
        
        // iOS-specific streaming parameters
        CONFIG.IOS_CHUNK_SIZE = 32768;            // 32KB chunks for iOS
        CONFIG.IOS_INITIAL_BUFFER = 65536;        // 64KB initial buffer
        CONFIG.IOS_MIN_BUFFER_TIME = 2;           // 2 seconds minimum buffer
        CONFIG.IOS_MAX_BUFFER_TIME = 10;          // 10 seconds maximum buffer
        
        logger.log('iOS streaming configuration adjusted', 'IOS_STREAM');
    }
    
    setupBufferHealthMonitoring() {
        this.bufferHealthTimer = setInterval(() => {
            if (!playerState.isPlaying || !playerState.audioElement) return;
            
            this.checkBufferHealth();
        }, 3000); // Check every 3 seconds for iOS
    }
    
    checkBufferHealth() {
        const audio = playerState.audioElement;
        if (!audio) return;
        
        const buffered = audio.buffered;
        const currentTime = audio.currentTime;
        const readyState = audio.readyState;
        const networkState = audio.networkState;
        
        // Calculate buffer ahead time
        let bufferAhead = 0;
        if (buffered.length > 0) {
            for (let i = 0; i < buffered.length; i++) {
                if (buffered.start(i) <= currentTime && buffered.end(i) > currentTime) {
                    bufferAhead = buffered.end(i) - currentTime;
                    break;
                }
            }
        }
        
        const bufferHealth = {
            bufferAhead: bufferAhead,
            readyState: readyState,
            networkState: networkState,
            paused: audio.paused,
            seeking: audio.seeking,
            stalled: readyState < HTMLMediaElement.HAVE_FUTURE_DATA
        };
        
        logger.log(`iOS Buffer: ${bufferAhead.toFixed(1)}s ahead, readyState: ${readyState}, networkState: ${networkState}`, 'IOS_BUFFER');
        
        // Handle various buffer issues
        if (bufferHealth.stalled && !audio.paused) {
            this.handleBufferStall(bufferHealth);
        } else if (bufferAhead < CONFIG.IOS_MIN_BUFFER_TIME && !audio.paused) {
            this.handleLowBuffer(bufferHealth);
        } else if (networkState === HTMLMediaElement.NETWORK_NO_SOURCE) {
            this.handleNoSource();
        }
    }
    
    handleBufferStall(bufferHealth) {
        logger.log(`iOS: Buffer stalled (readyState: ${bufferHealth.readyState})`, 'IOS_STREAM');
        
        // First attempt: try to resume playback
        if (this.fallbackAttempts === 0) {
            this.fallbackAttempts++;
            
            setTimeout(() => {
                if (playerState.audioElement && playerState.isPlaying) {
                    logger.log('iOS: Attempting to resume stalled stream', 'IOS_STREAM');
                    this.attemptBufferRecovery();
                }
            }, CONFIG.IOS_STALL_RECOVERY_DELAY);
            
        } else if (this.fallbackAttempts < this.maxFallbackAttempts) {
            // Second/third attempt: reload stream with fresh position
            this.fallbackAttempts++;
            
            setTimeout(() => {
                if (playerState.audioElement && playerState.isPlaying) {
                    logger.log(`iOS: Buffer stall recovery attempt ${this.fallbackAttempts}`, 'IOS_STREAM');
                    this.reloadStreamWithFreshPosition();
                }
            }, CONFIG.IOS_STALL_RECOVERY_DELAY * this.fallbackAttempts);
            
        } else {
            // Final attempt: full reconnection
            logger.log('iOS: Max buffer recovery attempts reached, triggering reconnection', 'IOS_STREAM');
            this.fallbackAttempts = 0;
            this.emit('forceReconnection', 'iOS buffer stall recovery failed');
        }
    }
    
    async attemptBufferRecovery() {
        const audio = playerState.audioElement;
        if (!audio) return;
        
        try {
            // Try to seek slightly forward to get past stall point
            const currentTime = audio.currentTime;
            audio.currentTime = currentTime + 0.1;
            
            // Wait a moment then try to play
            setTimeout(async () => {
                if (audio && playerState.isPlaying) {
                    try {
                        await audio.play();
                        logger.log('iOS: Buffer recovery successful', 'IOS_STREAM');
                        this.fallbackAttempts = 0; // Reset on success
                    } catch (error) {
                        logger.log(`iOS: Buffer recovery play failed: ${error.message}`, 'IOS_STREAM');
                    }
                }
            }, 500);
            
        } catch (error) {
            logger.log(`iOS: Buffer recovery failed: ${error.message}`, 'IOS_STREAM');
        }
    }
    
    async reloadStreamWithFreshPosition() {
        try {
            // Get fresh position from server
            const response = await fetch('/api/now-playing?ios_buffer_recovery=true', {
                headers: { 'Cache-Control': 'no-cache' }
            });
            
            if (response.ok) {
                const data = await response.json();
                const serverPosition = data.playback_position || 0;
                
                // Create new stream URL with fresh timestamp
                const timestamp = Date.now();
                const streamUrl = `/direct-stream?t=${timestamp}&position=${serverPosition}&platform=ios&buffer_recovery=true`;
                
                logger.log(`iOS: Reloading stream at position ${serverPosition}s`, 'IOS_STREAM');
                
                const audio = playerState.audioElement;
                if (audio) {
                    // Update the source
                    audio.src = streamUrl;
                    
                    // Update position tracking
                    playerState.clientStartTime = Date.now();
                    playerState.clientPositionOffset = serverPosition;
                    
                    // Try to start playback
                    setTimeout(async () => {
                        if (audio && playerState.isPlaying) {
                            try {
                                await audio.play();
                                logger.log('iOS: Stream reload successful', 'IOS_STREAM');
                                this.fallbackAttempts = Math.max(0, this.fallbackAttempts - 1); // Partial success
                            } catch (error) {
                                logger.log(`iOS: Stream reload play failed: ${error.message}`, 'IOS_STREAM');
                            }
                        }
                    }, 1000); // Give iOS time to process the new source
                }
            }
        } catch (error) {
            logger.log(`iOS: Stream reload failed: ${error.message}`, 'IOS_STREAM');
        }
    }
    
    handleLowBuffer(bufferHealth) {
        logger.log(`iOS: Low buffer detected (${bufferHealth.bufferAhead.toFixed(1)}s)`, 'IOS_BUFFER');
        
        // Don't immediately reconnect for low buffer, just monitor
        // iOS Safari will often recover on its own
        setTimeout(() => {
            this.checkBufferHealth();
        }, 2000);
    }
    
    handleNoSource() {
        logger.log('iOS: No source detected, triggering reconnection', 'IOS_STREAM');
        this.emit('forceReconnection', 'iOS no source');
    }
    
    setupStreamQualityMonitoring() {
        this.streamQualityTimer = setInterval(() => {
            if (!playerState.isPlaying || !playerState.audioElement) return;
            
            this.monitorStreamQuality();
        }, 10000); // Every 10 seconds
    }
    
    monitorStreamQuality() {
        const audio = playerState.audioElement;
        if (!audio) return;
        
        // Check for playback issues
        const quality = {
            paused: audio.paused,
            muted: audio.muted,
            volume: audio.volume,
            currentTime: audio.currentTime,
            duration: audio.duration || 0,
            networkState: audio.networkState,
            readyState: audio.readyState,
            playbackRate: audio.playbackRate
        };
        
        // Detect quality issues
        if (audio.paused && playerState.isPlaying) {
            logger.log('iOS: Unexpected pause detected', 'IOS_QUALITY');
            this.handleUnexpectedPause();
        }
        
        if (quality.playbackRate !== 1.0) {
            logger.log(`iOS: Abnormal playback rate: ${quality.playbackRate}`, 'IOS_QUALITY');
            audio.playbackRate = 1.0;
        }
        
        // Log quality periodically for debugging
        if (CONFIG.DEBUG_MODE) {
            logger.log(`iOS Quality: ${JSON.stringify(quality)}`, 'IOS_QUALITY');
        }
    }
    
    async handleUnexpectedPause() {
        const audio = playerState.audioElement;
        if (!audio || !playerState.isPlaying) return;
        
        logger.log('iOS: Attempting to recover from unexpected pause', 'IOS_QUALITY');
        
        try {
            await audio.play();
            logger.log('iOS: Recovered from unexpected pause', 'IOS_QUALITY');
        } catch (error) {
            logger.log(`iOS: Recovery from pause failed: ${error.message}`, 'IOS_QUALITY');
            
            // If recovery fails, try reloading the stream
            setTimeout(() => {
                if (playerState.isPlaying) {
                    this.reloadStreamWithFreshPosition();
                }
            }, 1000);
        }
    }
    
    setupIOSAudioOptimizations() {
        // Enhanced audio element configuration for iOS
        document.addEventListener('audioManager:audioCreated', (e) => {
            if (!playerState.isIOS) return;
            
            const audio = e.detail.audioElement;
            this.optimizeAudioElementForIOS(audio);
        });
    }
    
    optimizeAudioElementForIOS(audio) {
        if (!audio) return;
        
        logger.log('Optimizing audio element for iOS streaming', 'IOS_STREAM');
        
        // iOS-specific audio settings
        audio.preload = 'auto';  // Changed from 'metadata' to 'auto' for iOS
        audio.loop = false;
        audio.autoplay = false;
        
        // iOS Safari-specific attributes
        if (audio.setAttribute) {
            audio.setAttribute('webkit-playsinline', 'true');
            audio.setAttribute('playsinline', 'true');
            audio.setAttribute('x-webkit-airplay', 'allow');
        }
        
        // Disable picture-in-picture for iOS
        if ('disablePictureInPicture' in audio) {
            audio.disablePictureInPicture = true;
        }
        
        // Enhanced iOS event listeners
        audio.addEventListener('canplay', () => {
            logger.log('iOS: Audio can play (buffer sufficient)', 'IOS_STREAM');
            this.fallbackAttempts = 0; // Reset on successful buffer
        });
        
        audio.addEventListener('canplaythrough', () => {
            logger.log('iOS: Audio can play through (full buffer)', 'IOS_STREAM');
            this.fallbackAttempts = 0; // Reset on full buffer
        });
        
        audio.addEventListener('progress', () => {
            // iOS buffering progress
            const buffered = audio.buffered;
            if (buffered.length > 0) {
                const bufferedEnd = buffered.end(buffered.length - 1);
                const bufferedStart = buffered.start(0);
                const totalBuffered = bufferedEnd - bufferedStart;
                logger.log(`iOS: Buffer progress - ${totalBuffered.toFixed(1)}s buffered`, 'IOS_BUFFER');
            }
        });
        
        audio.addEventListener('suspend', () => {
            logger.log('iOS: Audio loading suspended by browser', 'IOS_STREAM');
            // iOS Safari sometimes suspends loading - try to resume
            setTimeout(() => {
                if (audio && playerState.isPlaying && audio.networkState === HTMLMediaElement.NETWORK_LOADING) {
                    audio.load(); // Force resume loading
                }
            }, 2000);
        });
        
        audio.addEventListener('abort', () => {
            logger.log('iOS: Audio loading aborted', 'IOS_STREAM');
            if (playerState.isPlaying) {
                // Try to recover from abort
                setTimeout(() => {
                    this.reloadStreamWithFreshPosition();
                }, 1000);
            }
        });
    }
    
    setupIOSChunkedLoading() {
        // Override the direct stream request for iOS to use chunked loading
        if (!playerState.isIOS) return;
        
        logger.log('Setting up iOS chunked loading optimization', 'IOS_STREAM');
        
        // This would integrate with the backend to request iOS-optimized chunks
        // For now, we'll ensure the stream URLs include iOS-specific parameters
    }
    
    // Create iOS-optimized stream URL
    createIOSStreamURL(position, timestamp) {
        let streamUrl = `/direct-stream?t=${timestamp}&position=${position}&platform=ios`;
        
        // Add iOS-specific parameters
        streamUrl += `&ios_optimized=true`;
        streamUrl += `&chunk_size=${CONFIG.IOS_CHUNK_SIZE}`;
        streamUrl += `&initial_buffer=${CONFIG.IOS_INITIAL_BUFFER}`;
        streamUrl += `&min_buffer_time=${CONFIG.IOS_MIN_BUFFER_TIME}`;
        
        // Add buffer recovery flag if this is a recovery attempt
        if (this.fallbackAttempts > 0) {
            streamUrl += `&buffer_recovery=${this.fallbackAttempts}`;
        }
        
        return streamUrl;
    }
    
    // Force immediate reconnection (bypasses normal reconnection logic)
    forceImmediateReconnection(reason) {
        logger.log(`iOS: Forcing immediate reconnection (${reason})`, 'IOS_STREAM');
        
        this.fallbackAttempts = 0; // Reset attempts
        
        // Clear any existing timers
        if (this.bufferHealthTimer) {
            clearInterval(this.bufferHealthTimer);
            this.bufferHealthTimer = null;
        }
        
        // Emit force reconnection
        this.emit('forceReconnection', `iOS immediate: ${reason}`);
        
        // Restart buffer monitoring after a delay
        setTimeout(() => {
            this.setupBufferHealthMonitoring();
        }, 5000);
    }
    
    // Event emitter
    emit(eventName, data) {
        const event = new CustomEvent(`iosStreaming:${eventName}`, { detail: data });
        document.dispatchEvent(event);
    }
    
    cleanup() {
        if (this.bufferHealthTimer) {
            clearInterval(this.bufferHealthTimer);
            this.bufferHealthTimer = null;
        }
        
        if (this.streamQualityTimer) {
            clearInterval(this.streamQualityTimer);
            this.streamQualityTimer = null;
        }
        
        this.fallbackAttempts = 0;
        
        logger.log('iOS streaming manager cleanup completed', 'IOS_STREAM');
    }
}

// Export singleton instance
export const iosStreamingManager = new IOSStreamingManager();