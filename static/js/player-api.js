// Updated player-api.js - Modified to avoid unnecessary position updates

// Fetch now playing info via API with return value for promise chaining
async function fetchNowPlaying() {
    try {
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            log(`Now playing API error: ${response.status}`, 'API', true);
            return {};
        }
        
        const data = await response.json();
        handleTrackInfoUpdate(data);
        
        // Return the data for promise chaining
        return data;
    } catch (error) {
        log(`Error fetching now playing: ${error.message}`, 'API', true);
        
        // Re-throw to allow proper promise handling
        throw error;
    }
}

// SIMPLIFIED: Handle track info updates without unnecessary position syncing
function handleTrackInfoUpdate(info) {
    try {
        // Check for error message
        if (info.error) {
            showStatus(`Server error: ${info.error}`, true);
            return;
        }
        
        // Get key info
        const position = info.playback_position || 0;
        
        // Store the server position for syncing
        state.serverPosition = position;
        
        // Check for track change
        const newTrackId = info.path;
        const isNewTrack = state.currentTrackId !== newTrackId;
        
        if (isNewTrack) {
            log(`Track changed to: ${info.title} by ${info.artist}`, 'TRACK');
            state.currentTrackId = newTrackId;
            
            // Store track changed time for end-of-track detection
            state.lastTrackChange = Date.now();
            
            // IMPORTANT: Only restart the stream on track change if we've been playing
            // for a while (to avoid restart loops)
            if (state.isPlaying && state.audioElement && !state.audioElement.paused && 
                state.audioElement.currentTime > 10 && 
                Date.now() - state.lastPositionSync > 30000) { // At least 30 seconds since last sync
                
                log('Track changed while playing, restarting stream to get new track', 'TRACK');
                restartDirectStream();
            }
        }
        
        // Update UI
        if (currentTitle) currentTitle.textContent = info.title || 'Unknown Title';
        if (currentArtist) currentArtist.textContent = info.artist || 'Unknown Artist';
        if (currentAlbum) currentAlbum.textContent = info.album || 'Unknown Album';
        
        // Store track duration for progress tracking and end detection
        if (info.duration) {
            state.trackPlaybackDuration = info.duration;
            if (currentDuration) currentDuration.textContent = formatTime(info.duration);
        }
        
        // SIMPLIFIED: Just update UI with current client position
        // DO NOT try to reset or sync audio position during regular polling
        if (state.audioElement && !state.audioElement.paused) {
            const currentTime = state.audioElement.currentTime;
            updateProgressBar(currentTime, state.trackPlaybackDuration);
        } else {
            // Use server's position only if we're not playing
            updateProgressBar(position, state.trackPlaybackDuration);
        }
        
        // Update listener count
        if (info.active_listeners !== undefined && listenerCount) {
            listenerCount.textContent = `Listeners: ${info.active_listeners}`;
        }
        
        // Update page title
        document.title = `${info.title} - ${info.artist} | ChillOut Radio`;
        
        // Update last track info time
        state.lastTrackInfoTime = Date.now();
    } catch (e) {
        log(`Error processing track info: ${e.message}`, 'TRACK', true);
    }
}

// Export functions
window.fetchNowPlaying = fetchNowPlaying;
window.handleTrackInfoUpdate = handleTrackInfoUpdate;