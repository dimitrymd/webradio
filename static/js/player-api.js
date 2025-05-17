// Fetch now playing info via API
async function fetchNowPlaying() {
    try {
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            log(`Now playing API error: ${response.status}`, 'API', true);
            return;
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

// Handle track info updates
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
            
            // If we're currently playing, restart the stream to get the new track
            if (state.isPlaying && state.audioElement && !state.audioElement.paused && state.audioElement.currentTime > 10) {
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
        
        // For streams that we're not seeing position updates,
        // estimate based on audioElement.currentTime
        if (state.audioElement && !state.audioElement.paused) {
            const currentTime = state.audioElement.currentTime;
            updateProgressBar(currentTime, state.trackPlaybackDuration);
        } else {
            // Use server's position
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