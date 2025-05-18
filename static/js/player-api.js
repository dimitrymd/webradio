// Updated player-api.js with track change detection improvements

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
        
        if (isNewTrack && state.currentTrackId && newTrackId) {
            log(`Track changed from ${state.currentTrackId} to ${newTrackId}`, 'TRACK');
            
            // If the track has changed and we're playing, we need to restart the stream
            if (state.isPlaying && typeof restartDirectStreamWithImprovedBuffering === 'function') {
                log('Track changed while stream is playing, restarting stream', 'TRACK');
                setTimeout(restartDirectStreamWithImprovedBuffering, 300);
            }
            
            // Store track changed time for end-of-track detection
            state.lastTrackChange = Date.now();
            
            // Reset playback position tracking
            state.serverPlayheadRate = null;
            
            // Always update UI for track change
            uiNeedsUpdate = true;
        }
        
        // Always update currentTrackId if we have a valid one
        if (newTrackId) {
            state.currentTrackId = newTrackId;
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
        
        // Update progress bar based on server position if needed
        if (state.audioElement && !state.audioElement.paused) {
            // We're playing audio, check if the server position is significantly different
            const clientPosition = state.audioElement.currentTime;
            const positionDifference = Math.abs(clientPosition - position);
            
            if (positionDifference > 10) {  // more than 10 seconds difference
                log(`Position significantly different: client=${clientPosition.toFixed(1)}s, server=${position}s (diff: ${positionDifference.toFixed(1)}s)`, 'POSITION');
                
                // If difference is large and not near end of track, force a seek
                if (clientPosition < duration * 0.9) {
                    log('Client significantly behind server, forcing seek', 'POSITION');
                    
                    if (typeof forceSeekToServerPosition === 'function') {
                        setTimeout(forceSeekToServerPosition, 500);
                    } else {
                        try {
                            state.audioElement.currentTime = position;
                        } catch (e) {
                            log(`Error seeking: ${e.message}`, 'POSITION', true);
                        }
                    }
                }
            }
            
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
        } else {
            // We're not playing or just starting, use server position directly
            updateProgressBar(position, duration);
        }
        
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