// Elements
const startBtn = document.getElementById('start-btn');
const muteBtn = document.getElementById('mute-btn');
const volumeControl = document.getElementById('volume');
const statusMessage = document.getElementById('status-message');
const listenerCount = document.getElementById('listener-count');

// Current track display elements
const currentTitle = document.getElementById('current-title');
const currentArtist = document.getElementById('current-artist');
const currentAlbum = document.getElementById('current-album');
const currentDuration = document.getElementById('current-duration');

// WebSocket and audio context
let ws = null;
let audioContext = null;
let gainNode = null;
let isPlaying = false;
let isMuted = false;

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

// Update now playing information
async function updateNowPlaying() {
    try {
        console.log("Fetching now playing info...");
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            throw new Error(`Failed to fetch now playing info: ${response.status}`);
        }
        
        const data = await response.json();
        console.log("Received now playing info:", data);
        
        if (data.error) {
            currentTitle.textContent = 'No tracks available';
            currentArtist.textContent = 'Please add MP3 files to the server';
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
            
            // Update page title
            document.title = `${data.title} - ${data.artist} | Rust Web Radio`;
        }
    } catch (error) {
        console.error('Error fetching now playing:', error);
        showStatus('Error updating now playing information', true);
    }
}

// Initialize audio
function initAudio() {
    if (audioContext) {
        return true; // Already initialized
    }
    
    try {
        // Create AudioContext
        audioContext = new (window.AudioContext || window.webkitAudioContext)();
        
        // Create gain node for volume control
        gainNode = audioContext.createGain();
        gainNode.gain.value = volumeControl.value;
        gainNode.connect(audioContext.destination);
        
        console.log("AudioContext initialized");
        return true;
    } catch (e) {
        console.error('Failed to create AudioContext:', e);
        showStatus('Your browser does not support Web Audio API', true);
        return false;
    }
}

// Start audio streaming
function startAudio() {
    console.log('Starting audio playback');
    startBtn.disabled = true;
    
    // Initialize audio if needed
    if (!initAudio()) {
        startBtn.disabled = false;
        return;
    }
    
    // Connect to WebSocket
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/stream`;
    console.log(`Connecting to WebSocket: ${wsUrl}`);
    
    ws = new WebSocket(wsUrl);
    
    ws.onopen = () => {
        console.log('WebSocket connection established');
        showStatus('Connected to audio stream');
        startBtn.textContent = 'Disconnect';
        startBtn.disabled = false;
        startBtn.dataset.connected = 'true';
        isPlaying = true;
    };
    
    ws.onmessage = async (event) => {
        // Handle binary audio data
        if (event.data instanceof Blob) {
            if (!isPlaying) return;
            
            try {
                // Get ArrayBuffer from Blob
                const arrayBuffer = await event.data.arrayBuffer();
                
                // Decode audio data
                audioContext.decodeAudioData(arrayBuffer, (buffer) => {
                    if (!isPlaying) return;
                    
                    // Create buffer source
                    const source = audioContext.createBufferSource();
                    source.buffer = buffer;
                    
                    // Connect to gain node
                    source.connect(gainNode);
                    
                    // Start playback
                    source.start(0);
                    
                }, (error) => {
                    console.error('Error decoding audio:', error);
                });
            } catch (error) {
                console.error('Error processing audio chunk:', error);
            }
        } 
        // Handle text message (track info)
        else {
            try {
                console.log('Received track info:', event.data);
                const trackInfo = JSON.parse(event.data);
                
                // Update track info
                currentTitle.textContent = trackInfo.title || 'Unknown Title';
                currentArtist.textContent = trackInfo.artist || 'Unknown Artist';
                currentAlbum.textContent = trackInfo.album || 'Unknown Album';
                currentDuration.textContent = formatTime(trackInfo.duration);
                
                // Update page title
                document.title = `${trackInfo.title} - ${trackInfo.artist} | Rust Web Radio`;
            } catch (error) {
                console.error('Error parsing track info:', error);
            }
        }
    };
    
    ws.onclose = (event) => {
        console.log(`WebSocket connection closed: Code ${event.code}`);
        
        isPlaying = false;
        
        // Only show disconnection message if it wasn't requested by the user
        if (startBtn.dataset.connected === 'true') {
            showStatus('Disconnected from audio stream', true);
            
            // Try to reconnect automatically
            setTimeout(() => {
                if (startBtn.dataset.connected === 'true') {
                    console.log('Attempting to reconnect...');
                    startAudio();
                }
            }, 2000);
        }
        
        startBtn.textContent = 'Connect';
        startBtn.disabled = false;
        startBtn.dataset.connected = 'false';
    };
    
    ws.onerror = (error) => {
        console.error('WebSocket error:', error);
        showStatus('Error connecting to audio stream', true);
        startBtn.textContent = 'Connect';
        startBtn.disabled = false;
        startBtn.dataset.connected = 'false';
    };
}

// Stop audio streaming
function stopAudio() {
    console.log('Stopping audio playback');
    
    isPlaying = false;
    
    // Close WebSocket
    if (ws) {
        ws.close();
        ws = null;
    }
    
    showStatus('Disconnected from audio stream');
    startBtn.textContent = 'Connect';
    startBtn.disabled = false;
    startBtn.dataset.connected = 'false';
}

// Toggle connection
function toggleConnection() {
    const isConnected = startBtn.dataset.connected === 'true';
    
    if (isConnected) {
        stopAudio();
    } else {
        startAudio();
    }
}

// Toggle mute/unmute
function toggleMute() {
    if (!gainNode) return;
    
    isMuted = !isMuted;
    
    if (isMuted) {
        gainNode.gain.value = 0;
        muteBtn.textContent = 'Unmute';
    } else {
        gainNode.gain.value = volumeControl.value;
        muteBtn.textContent = 'Mute';
    }
}

// Update stats
async function updateStats() {
    try {
        const response = await fetch('/api/stats');
        const data = await response.json();
        listenerCount.textContent = `Listeners: ${data.active_listeners}`;
    } catch (error) {
        console.error('Error fetching stats:', error);
    }
}

// Event listeners
startBtn.addEventListener('click', toggleConnection);
muteBtn.addEventListener('click', toggleMute);
volumeControl.addEventListener('input', () => {
    if (gainNode && !isMuted) {
        gainNode.gain.value = volumeControl.value;
    }
    
    localStorage.setItem('radioVolume', volumeControl.value);
});

// Handle page visibility
document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') {
        console.log('Page is now visible');
        
        // Update now playing
        updateNowPlaying();
        
        // Reconnect if needed
        if (startBtn.dataset.connected === 'true' && (!ws || ws.readyState !== WebSocket.OPEN)) {
            console.log('Reconnecting after page became visible');
            startAudio();
        }
    }
});

// Initialize
document.addEventListener('DOMContentLoaded', async () => {
    console.log('Page loaded, initializing...');
    
    // Set initial button state
    startBtn.textContent = 'Connect';
    startBtn.dataset.connected = 'false';
    
    // Set initial volume
    const savedVolume = localStorage.getItem('radioVolume');
    if (savedVolume !== null) {
        volumeControl.value = savedVolume;
    }
    
    // Update now playing
    await updateNowPlaying();
    
    // Auto-update
    setInterval(updateNowPlaying, 5000);
    setInterval(updateStats, 10000);
    
    console.log('Initialization complete');
});