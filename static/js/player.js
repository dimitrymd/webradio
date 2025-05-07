// Elements
const startBtn = document.getElementById('start-btn');
const muteBtn = document.getElementById('mute-btn');
const nextBtn = document.getElementById('next-btn');
const volumeControl = document.getElementById('volume');
const scanBtn = document.getElementById('scan-btn');
const shuffleBtn = document.getElementById('shuffle-btn');
const playlistContainer = document.getElementById('playlist-container');
const statusMessage = document.getElementById('status-message');
const listenerCount = document.getElementById('listener-count');
const directAudio = document.getElementById('direct-audio');

// Current track display elements
const currentTitle = document.getElementById('current-title');
const currentArtist = document.getElementById('current-artist');
const currentAlbum = document.getElementById('current-album');
const currentDuration = document.getElementById('current-duration');

// WebSocket connection
let ws = null;
let audioElement = null;
let isMuted = false;
let previousVolume = 0.7; // Default volume

// Debug flag - set to true for console logging
const DEBUG = true;

// Format time (seconds to MM:SS)
function formatTime(seconds) {
    if (!seconds) return '0:00';
    const minutes = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

// Show status message
function showStatus(message, isError = false) {
    statusMessage.textContent = message;
    statusMessage.style.display = 'block';
    statusMessage.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
    
    // Hide after 3 seconds
    setTimeout(() => {
        statusMessage.style.display = 'none';
    }, 3000);
}

// Debug logging
function debugLog(message) {
    if (DEBUG) {
        console.log(`[DEBUG] ${message}`);
    }
}

// Update now playing information
async function updateNowPlaying() {
    try {
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            throw new Error('Failed to fetch now playing info');
        }
        
        const data = await response.json();
        
        if (data.error) {
            currentTitle.textContent = 'No tracks available';
            currentArtist.textContent = 'Please scan for MP3 files';
            currentAlbum.textContent = '';
            currentDuration.textContent = '';
        } else {
            currentTitle.textContent = data.title || 'Unknown Title';
            currentArtist.textContent = data.artist || 'Unknown Artist';
            currentAlbum.textContent = data.album || 'Unknown Album';
            currentDuration.textContent = formatTime(data.duration);
            
            // Update listener count if available
            if (data.active_listeners !== undefined) {
                listenerCount.textContent = `Listeners: ${data.active_listeners}`;
            }
            
            // Update active track in playlist
            const tracks = document.querySelectorAll('.track');
            tracks.forEach(track => {
                track.classList.remove('active');
                if (track.dataset.path === data.path) {
                    track.classList.add('active');
                    // Scroll to current track
                    track.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
                }
            });
            
            // Update page title
            document.title = `${data.title} - ${data.artist} | Rust Web Radio`;
        }
    } catch (error) {
        console.error('Error fetching now playing:', error);
        showStatus('Error updating now playing information', true);
    }
}

// Load playlist
async function loadPlaylist() {
    try {
        const response = await fetch('/api/playlist');
        if (!response.ok) {
            throw new Error('Failed to fetch playlist');
        }
        
        const data = await response.json();
        
        playlistContainer.innerHTML = '';
        
        if (data.tracks.length === 0) {
            playlistContainer.innerHTML = '<div class="status-message">No tracks found. Click "Scan for MP3s" to find music files.</div>';
            return;
        }
        
        data.tracks.forEach((track, index) => {
            const trackElement = document.createElement('div');
            trackElement.className = 'track';
            if (index === data.current_track) {
                trackElement.classList.add('active');
            }
            trackElement.dataset.path = track.path;
            trackElement.dataset.index = index;
            
            trackElement.innerHTML = `
                <div class="track-title">${track.title || 'Unknown Title'}</div>
                <div class="track-artist">${track.artist || 'Unknown Artist'}</div>
                <div class="duration">${formatTime(track.duration)}</div>
            `;
            
            trackElement.addEventListener('click', () => {
                playSongAt(index);
            });
            
            playlistContainer.appendChild(trackElement);
        });
    } catch (error) {
        console.error('Error loading playlist:', error);
        showStatus('Error loading playlist', true);
    }
}

// Connect to WebSocket and start streaming
function connectWebSocket() {
    // Close any existing connection
    if (ws) {
        ws.close();
    }
    
    // Create audio element if it doesn't exist
    if (!audioElement) {
        audioElement = new Audio('/stream.mp3');
        audioElement.volume = volumeControl.value;
        
        // Add event listeners for debugging
        audioElement.addEventListener('playing', () => debugLog('Audio started playing'));
        audioElement.addEventListener('pause', () => debugLog('Audio paused'));
        audioElement.addEventListener('error', (e) => console.error('Audio error:', e));
        audioElement.addEventListener('stalled', () => debugLog('Audio stalled'));
        audioElement.addEventListener('waiting', () => debugLog('Audio waiting'));
    }
    
    // Create a new WebSocket connection
    const wsUrl = `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/stream`;
    debugLog(`Connecting to WebSocket at ${wsUrl}`);
    
    ws = new WebSocket(wsUrl);
    
    // Handle WebSocket events
    ws.onopen = () => {
        debugLog('WebSocket connection established');
        showStatus('Connected to audio stream');
        
        // Update button state
        startBtn.disabled = true;
        startBtn.textContent = 'Connected';
        
        // Start playing the audio
        audioElement.play()
            .then(() => debugLog('Audio playback started'))
            .catch(err => {
                console.error('Audio playback failed:', err);
                showStatus('Failed to start audio playback. Try the direct player below.', true);
            });
    };
    
    ws.onmessage = (event) => {
        if (event.data instanceof Blob) {
            debugLog(`Received binary data of size: ${event.data.size} bytes`);
        } else {
            // Text data - track info
            debugLog(`Received text message: ${event.data}`);
            try {
                const trackInfo = JSON.parse(event.data);
                currentTitle.textContent = trackInfo.title || 'Unknown Title';
                currentArtist.textContent = trackInfo.artist || 'Unknown Artist';
                currentAlbum.textContent = trackInfo.album || 'Unknown Album';
                currentDuration.textContent = formatTime(trackInfo.duration);
                
                // Update page title
                document.title = `${trackInfo.title} - ${trackInfo.artist} | Rust Web Radio`;
                
                // Reload audio to get the new track
                audioElement.load();
                audioElement.play()
                    .then(() => debugLog('Track changed, playback resumed'))
                    .catch(err => console.error('Error resuming after track change:', err));
            } catch (error) {
                console.error('Error parsing track info:', error);
            }
        }
    };
    
    ws.onclose = () => {
        debugLog('WebSocket connection closed');
        
        // Pause audio
        if (audioElement) {
            audioElement.pause();
        }
        
        // Update button state
        startBtn.disabled = false;
        startBtn.textContent = 'Start Listening';
        showStatus('Connection closed. Click Start to reconnect.', true);
    };
    
    ws.onerror = (error) => {
        console.error('WebSocket error:', error);
        debugLog(`WebSocket error: ${error}`);
        showStatus('Error connecting to audio stream', true);
    };
}

// Initialize audio and start streaming
function startAudio() {
    debugLog('Starting audio playback');
    connectWebSocket();
}

// Toggle mute/unmute
function toggleMute() {
    if (audioElement) {
        if (isMuted) {
            // Unmute
            audioElement.volume = previousVolume;
            volumeControl.value = previousVolume;
            muteBtn.textContent = 'Mute';
            isMuted = false;
            debugLog('Unmuted audio');
        } else {
            // Mute
            previousVolume = audioElement.volume;
            audioElement.volume = 0;
            volumeControl.value = 0;
            muteBtn.textContent = 'Unmute';
            isMuted = true;
            debugLog('Muted audio');
        }
    }
    
    // Also mute/unmute the direct audio player
    if (directAudio) {
        directAudio.volume = isMuted ? 0 : previousVolume;
    }
}

// Play song at specific index
async function playSongAt(index) {
    try {
        debugLog(`Switching to track at index ${index}`);
        const response = await fetch(`/api/playlist/play/${index}`, {
            method: 'POST'
        });
        
        if (!response.ok) {
            throw new Error('Failed to set track');
        }
        
        // Reload audio to get the new track
        if (audioElement) {
            audioElement.load();
            audioElement.play()
                .catch(err => console.error('Error playing after track change:', err));
        }
        
        // Also reload the direct audio player
        if (directAudio) {
            directAudio.load();
        }
        
        // Update now playing info
        await updateNowPlaying();
    } catch (error) {
        console.error('Error setting track:', error);
        showStatus('Error changing track', true);
    }
}

// Next track
async function nextTrack() {
    try {
        debugLog('Switching to next track');
        nextBtn.disabled = true;
        
        const response = await fetch('/api/next', {
            method: 'POST'
        });
        
        if (!response.ok) {
            throw new Error('Failed to advance track');
        }
        
        // Reload audio to get the new track
        if (audioElement) {
            audioElement.load();
            audioElement.play()
                .catch(err => console.error('Error playing after next track:', err));
        }
        
        // Also reload the direct audio player
        if (directAudio) {
            directAudio.load();
        }
        
        // Update now playing info
        await updateNowPlaying();
    } catch (error) {
        console.error('Error advancing track:', error);
        showStatus('Error changing track', true);
    } finally {
        nextBtn.disabled = false;
    }
}

// Scan for MP3 files
async function scanMusic() {
    try {
        scanBtn.disabled = true;
        scanBtn.textContent = 'Scanning...';
        
        const response = await fetch('/api/playlist/scan', {
            method: 'POST'
        });
        
        if (!response.ok) {
            throw new Error('Scan failed');
        }
        
        const data = await response.json();
        
        showStatus(data.message);
        
        // Reload playlist
        await loadPlaylist();
        
        // Update now playing
        await updateNowPlaying();
    } catch (error) {
        console.error('Error scanning music:', error);
        showStatus('Error scanning for music files', true);
    } finally {
        scanBtn.disabled = false;
        scanBtn.textContent = 'Scan for MP3s';
    }
}

// Shuffle playlist
async function shufflePlaylist() {
    try {
        shuffleBtn.disabled = true;
        
        const response = await fetch('/api/playlist/shuffle', {
            method: 'POST'
        });
        
        if (!response.ok) {
            throw new Error('Shuffle failed');
        }
        
        // Reload playlist
        await loadPlaylist();
        
        showStatus('Playlist shuffled');
    } catch (error) {
        console.error('Error shuffling playlist:', error);
        showStatus('Error shuffling playlist', true);
    } finally {
        shuffleBtn.disabled = false;
    }
}

// Update stats every 10 seconds
async function updateStats() {
    try {
        const response = await fetch('/api/stats');
        const data = await response.json();
        listenerCount.textContent = `Listeners: ${data.active_listeners} / ${data.max_concurrent_users}`;
    } catch (error) {
        console.error('Error fetching stats:', error);
    }
}

// Event listeners
startBtn.addEventListener('click', startAudio);
muteBtn.addEventListener('click', toggleMute);
nextBtn.addEventListener('click', nextTrack);
volumeControl.addEventListener('input', () => {
    if (audioElement) {
        audioElement.volume = volumeControl.value;
    }
    
    // Also update direct audio player
    if (directAudio) {
        directAudio.volume = volumeControl.value;
    }
    
    // Update mute state
    if (volumeControl.value > 0) {
        isMuted = false;
        muteBtn.textContent = 'Mute';
    } else {
        isMuted = true;
        muteBtn.textContent = 'Unmute';
    }
    
    // Save volume preference in local storage
    localStorage.setItem('radioVolume', volumeControl.value);
});

scanBtn.addEventListener('click', scanMusic);
shuffleBtn.addEventListener('click', shufflePlaylist);

// Initialize
document.addEventListener('DOMContentLoaded', async () => {
    debugLog('Page loaded, initializing...');
    
    // Set initial volume (from local storage if available)
    const savedVolume = localStorage.getItem('radioVolume');
    if (savedVolume !== null) {
        volumeControl.value = savedVolume;
        if (directAudio) {
            directAudio.volume = savedVolume;
        }
    }
    
    // Load playlist
    await loadPlaylist();
    
    // Update now playing
    await updateNowPlaying();
    
    // Auto-update now playing every 10 seconds
    setInterval(updateNowPlaying, 10000);
    
    // Auto-update stats every 10 seconds
    setInterval(updateStats, 10000);
    
    debugLog('Initialization complete');
});