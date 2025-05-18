// player-api.js - Improved track info handling and error recovery

// Fetch now playing info via API with improved error handling and retries
async function fetchNowPlaying(retryCount = 2) {
    try {
        const controller = new AbortController();
        const signal = controller.signal;
        
        // Set timeout to abort long requests
        const timeoutId = setTimeout(() => controller.abort(), 5000);
        
        const response = await fetch('/api/now-playing', { signal });
        
        // Clear the timeout
        clearTimeout(timeoutId);
        
        if (!response.ok) {
            log(`Now playing API error: ${response.status}`, 'API', true);
            
            if (retryCount > 0 && response.status >= 500) {
                // Retry server errors after a delay
                log(`Retrying now playing fetch, ${retryCount} attempts left`, 'API');
                await new Promise(resolve => setTimeout(resolve, 1000));
                return fetchNowPlaying(retryCount - 1);
            }
            
            return {};
        }
        
        const data = await response.json();
        
        // Only process valid data
        if (data && !data.error) {
            handleTrackInfoUpdate(data);
        } else if (data.error) {
            log(`Server reported error: ${data.error}`, 'API', true);
        }
        
        // Return the data for promise chaining
        return data;
    } catch (error) {
        // Handle specific error types
        if (error.name === 'AbortError') {
            log('Now playing request timed out', 'API', true);
        } else {
            log(`Error fetching now playing: ${error.message}`, 'API', true);
        }
        
        // Retry network errors
        if (retryCount > 0 && (error.name === 'TypeError' || error.name === 'AbortError')) {
            log(`Retrying now playing fetch, ${retryCount} attempts left`, 'API');
            await new Promise(resolve => setTimeout(resolve, 1000));
            return fetchNowPlaying(retryCount - 1);
        }
        
        // Re-throw to allow proper promise handling
        throw error;
    }
}

// Enhanced track info handling with better track change detection
function handleTrackInfoUpdate(info) {
    try {
        // Check for error message
        if (info.error) {
            showStatus(`Server error: ${info.error}`, true);
            return;
        }
        
        // Get key info
        const position = info.playback_position || 0;
        const duration = info.duration || 0;
        
        // Store the server position for syncing
        state.serverPosition = position;
        state.serverPositionTime = Date.now();
        
        // Only update UI if data has actually changed to prevent unnecessary reflows
        let uiNeedsUpdate = false;
        
        // Check for track change
        const newTrackId = info.path;
        const isNewTrack = state.currentTrackId !== newTrackId;
        
        if (isNewTrack) {
            log(`Track changed to: "${info.title}" by "${info.artist}"`, 'TRACK');
            state.currentTrackId = newTrackId;
            
            // Store track changed time for end-of-track detection
            state.lastTrackChange = Date.now();
            
            // Reset playback position tracking
            state.serverPlayheadRate = null;
            
            // Always update UI for track change
            uiNeedsUpdate = true;
            
            // Track transitions need special handling
            if (state.isPlaying && state.audioElement && !state.audioElement.paused) {
                // Only handle track change automatically if we've been playing for a while
                // to avoid restart loops
                const playbackTime = (Date.now() - state.streamStartTime) / 1000;
                
                if (playbackTime > config.MIN_TRACK_PLAYBACK_TIME) {
                    if (typeof window.restartDirectStreamWithImprovedBuffering === 'function') {
                        log('Track changed while playing, scheduling restart to get new track', 'TRACK');
                        setTimeout(restartDirectStreamWithImprovedBuffering, 1000);
                    }
                } else {
                    log(`Track changed but we've only been playing for ${playbackTime.toFixed(1)}s, continuing playback`, 'TRACK');
                }
            }
        }
        
        // Check if any track metadata has changed
        if (currentTitle && currentTitle.textContent !== (info.title || 'Unknown Title')) {
            currentTitle.textContent = info.title || 'Unknown Title';
            uiNeedsUpdate = true;
        }
        
        if (currentArtist && currentArtist.textContent !== (info.artist || 'Unknown Artist')) {
            currentArtist.textContent = info.artist || 'Unknown Artist';
            uiNeedsUpdate = true;
        }
        
        if (currentAlbum && currentAlbum.textContent !== (info.album || 'Unknown Album')) {
            currentAlbum.textContent = info.album || 'Unknown Album';
            uiNeedsUpdate = true;
        }
        
        // Store track duration for progress tracking and end detection
        if (info.duration && info.duration !== state.trackDuration) {
            state.trackDuration = info.duration;
            state.trackPlaybackDuration = info.duration;
            
            if (currentDuration) {
                currentDuration.textContent = formatTime(info.duration);
            }
            
            uiNeedsUpdate = true;
        }
        
        // Check if we need to update progress bar
        let clientPosition = 0;
        
        if (state.audioElement && !state.audioElement.paused) {
            // We're playing audio, get client position
            clientPosition = state.audioElement.currentTime;
            
            // Check if server position is significantly different from client
            const positionDifference = Math.abs(clientPosition - position);
            
            if (positionDifference > config.POSITION_SYNC_THRESHOLD) {
                log(`Position significantly different: client=${clientPosition.toFixed(1)}s, server=${position}s (diff: ${positionDifference.toFixed(1)}s)`, 'POSITION');
                
                // Update our tracking metrics but don't disturb playback
                state.serverPlayheadDifference = positionDifference;
                
                // Calculate server playhead rate (how many seconds per second)
                if (state.serverPosition !== undefined && state.serverPositionTime) {
                    const timeDelta = (Date.now() - state.serverPositionTime) / 1000;
                    if (timeDelta > 0) {
                        const positionDelta = position - state.serverPosition;
                        state.serverPlayheadRate = positionDelta / timeDelta;
                        
                        // Log if rate is unexpected
                        if (Math.abs(state.serverPlayheadRate - 1.0) > 0.1) {
                            log(`Server playhead rate: ${state.serverPlayheadRate.toFixed(2)}x`, 'POSITION');
                        }
                    }
                }
            }
        } else {
            // We're not playing, use server position
            clientPosition = position;
        }
        
        // Always update progress bar for smooth UX
        updateProgressBar(clientPosition, state.trackDuration);
        
        // Update listener count if changed
        if (info.active_listeners !== undefined && listenerCount) {
            const currentCount = listenerCount.textContent;
            const newCount = `Listeners: ${info.active_listeners}`;
            
            if (currentCount !== newCount) {
                listenerCount.textContent = newCount;
            }
        }
        
        // Update page title for better UX
        document.title = `${info.title} - ${info.artist} | ChillOut Radio`;
        
        // Update last track info time
        state.lastTrackInfoTime = Date.now();
        
        // If UI was updated, log it
        if (uiNeedsUpdate) {
            log(`Updated player UI with track info: "${info.title}" (${info.duration}s)`, 'UI');
        }
    } catch (e) {
        log(`Error processing track info: ${e.message}`, 'TRACK', true);
    }
}

// Export functions
window.fetchNowPlaying = fetchNowPlaying;
window.handleTrackInfoUpdate = handleTrackInfoUpdate;