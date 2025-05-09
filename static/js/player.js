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
const currentPosition = document.getElementById('current-position');

// WebSocket and audio context
let ws = null;
let audioContext = null;
let mediaSource = null;
let sourceBuffer = null;
let audioQueue = [];
let isPlaying = false;
let isMuted = false;
let reconnectAttempts = 0;
let maxReconnectAttempts = 5;
let connectionTimeout = null;
let directAudio = null;
let checkNowPlayingInterval = null;
let audioLastUpdateTime = Date.now();
let isProcessingQueue = false;

// Format time (seconds to MM:SS)
function formatTime(seconds) {
    if (!seconds) return '0:00';
    const minutes = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

// Update the progress bar
function updateProgressBar(position, duration) {
    const progressBar = document.getElementById('progress-bar');
    if (progressBar && duration > 0) {
        const percent = (position / duration) * 100;
        progressBar.style.width = `${percent}%`;
    }
}

// Show status message
function showStatus(message, isError = false) {
    statusMessage.textContent = message;
    statusMessage.style.display = 'block';
    statusMessage.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
    
    // Hide after 3 seconds for non-errors
    if (!isError) {
        setTimeout(() => {
            statusMessage.style.display = 'none';
        }, 3000);
    }
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
            currentPosition.textContent = '';
        } else {
            // Store track ID (path) for change detection
            const newTrackId = data.path;
            const trackChanged = currentTitle.dataset.trackId !== newTrackId;
            
            if (trackChanged) {
                console.log("Track changed, updating display");
                
                // Update track ID
                currentTitle.dataset.trackId = newTrackId;
                
                // Clear progress bar
                updateProgressBar(0, data.duration);
            }
            
            currentTitle.textContent = data.title || 'Unknown Title';
            currentArtist.textContent = data.artist || 'Unknown Artist';
            currentAlbum.textContent = data.album || 'Unknown Album';
            currentDuration.textContent = formatTime(data.duration);
            
            // Update position if available
            if (data.playback_position !== undefined) {
                currentPosition.textContent = formatTime(data.playback_position);
                updateProgressBar(data.playback_position, data.duration);
            }
            
            // Update listener count if available
            if (data.active_listeners !== undefined) {
                listenerCount.textContent = `Listeners: ${data.active_listeners}`;
            }
            
            // Update page title
            document.title = `${data.title} - ${data.artist} | Rust Web Radio`;
            
            // Update the last update time
            audioLastUpdateTime = Date.now();
        }
    } catch (error) {
        console.error('Error fetching now playing:', error);
        showStatus('Error updating now playing information', true);
    }
}

// Update stats
async function updateStats() {
    try {
        const response = await fetch('/api/stats');
        const data = await response.json();
        
        // Update listener count
        listenerCount.textContent = `Listeners: ${data.active_listeners}`;
    } catch (error) {
        console.error('Error fetching stats:', error);
    }
}

// Start audio streaming
function startAudio() {
    console.log('Starting audio playback');
    startBtn.disabled = true;
    
    // Reset reconnect attempts
    reconnectAttempts = 0;
    
    // Determine best streaming method based on browser capabilities
    if ('MediaSource' in window && MediaSource.isTypeSupported('audio/mpeg')) {
        console.log('Using MediaSource API for streaming');
        startMSEStreaming();
    } else {
        console.log('Using direct HTTP streaming');
        startDirectStreaming();
    }
    
    // Start frequent checks of now playing info
    if (checkNowPlayingInterval) {
        clearInterval(checkNowPlayingInterval);
    }
    checkNowPlayingInterval = setInterval(updateNowPlaying, 2000);
}

// WebSocket streaming with MediaSource API
function startMSEStreaming() {
    // Create audio element if needed
    if (!directAudio) {
        directAudio = document.createElement('audio');
        directAudio.autoplay = true;
        directAudio.controls = false;
        directAudio.style.display = 'none';
        document.body.appendChild(directAudio);
        
        // Set initial volume
        directAudio.volume = volumeControl.value;
    }
    
    // Create Media Source
    mediaSource = new MediaSource();
    directAudio.src = URL.createObjectURL(mediaSource);
    
    // Handle Media Source open event
    mediaSource.addEventListener('sourceopen', function() {
        // Create source buffer
        try {
            sourceBuffer = mediaSource.addSourceBuffer('audio/mpeg');
            
            // Handle update end events
            sourceBuffer.addEventListener('updateend', function() {
                // Process the next item in the queue if available
                processQueue();
            });
            
            // Connect to WebSocket
            connectWebSocket();
        } catch (e) {
            console.error('Error setting up MSE:', e);
            // Fall back to direct streaming if MSE fails
            startDirectStreaming();
        }
    });
    
    // Set up timeout for initial connection
    connectionTimeout = setTimeout(function() {
        console.log('Connection timeout for MSE');
        // Fall back to direct streaming if MSE times out
        startDirectStreaming();
    }, 5000);
}

// Process the audio queue
function processQueue() {
    if (audioQueue.length > 0 && !isProcessingQueue && sourceBuffer && !sourceBuffer.updating) {
        isProcessingQueue = true;
        const data = audioQueue.shift();
        try {
            sourceBuffer.appendBuffer(data);
        } catch (e) {
            console.error('Error appending buffer:', e);
            // If we hit a quota exceeded error, clear the buffer and try again
            if (e.name === 'QuotaExceededError') {
                console.log('Buffer full, removing old data');
                // Remove 10 seconds from the beginning
                if (sourceBuffer.buffered.length > 0) {
                    const start = sourceBuffer.buffered.start(0);
                    const end = start + 10;
                    sourceBuffer.remove(start, end);
                }
                // Put the data back in the queue
                audioQueue.unshift(data);
            }
        }
        isProcessingQueue = false;
    }
}

// Connect to WebSocket for streaming
function connectWebSocket() {
    // Clean up any existing WebSocket
    if (ws) {
        ws.close();
        ws = null;
    }
    
    // Determine the WebSocket URL
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/stream`;
    console.log(`Connecting to WebSocket: ${wsUrl}`);
    
    // Create new WebSocket
    ws = new WebSocket(wsUrl);
    
    // Set up event handlers
    ws.onopen = function() {
        console.log('WebSocket connection established');
        showStatus('Connected to audio stream');
        startBtn.textContent = 'Disconnect';
        startBtn.disabled = false;
        startBtn.dataset.connected = 'true';
        isPlaying = true;
        
        // Clear connection timeout if set
        if (connectionTimeout) {
            clearTimeout(connectionTimeout);
            connectionTimeout = null;
        }
    };
    
    ws.onmessage = function(event) {
        // Clear connection timeout if set
        if (connectionTimeout) {
            clearTimeout(connectionTimeout);
            connectionTimeout = null;
        }
        
        // Reset the audioLastUpdateTime
        audioLastUpdateTime = Date.now();
        
        // Process the received data
        if (event.data instanceof Blob) {
            // Handle binary audio data
            event.data.arrayBuffer().then(buffer => {
                if (sourceBuffer && mediaSource.readyState === 'open') {
                    // Add to queue
                    audioQueue.push(buffer);
                    
                    // Process queue if not already processing
                    if (!isProcessingQueue && !sourceBuffer.updating) {
                        processQueue();
                    }
                }
            }).catch(e => {
                console.error('Error processing audio data:', e);
            });
        } else {
            // Handle text data (track info)
            try {
                const info = JSON.parse(event.data);
                console.log('Received track info:', info);
                
                // Update display
                currentTitle.textContent = info.title || 'Unknown Title';
                currentArtist.textContent = info.artist || 'Unknown Artist';
                currentAlbum.textContent = info.album || 'Unknown Album';
                currentDuration.textContent = formatTime(info.duration);
                
                // Store track ID
                currentTitle.dataset.trackId = info.path;
                
                // Update page title
                document.title = `${info.title} - ${info.artist} | Rust Web Radio`;
            } catch (e) {
                console.error('Error parsing track info:', e);
            }
        }
    };
    
    ws.onclose = function(event) {
        console.log(`WebSocket connection closed: Code ${event.code}`);
        
        // Only attempt reconnect if it wasn't requested by the user
        if (startBtn.dataset.connected === 'true' && isPlaying) {
            handleStreamError('Connection lost. Attempting to reconnect...');
        }
    };
    
    ws.onerror = function(error) {
        console.error('WebSocket error:', error);
        handleStreamError('Error connecting to audio stream');
    };
}

// Direct HTTP streaming
function startDirectStreaming() {
    // Create an audio element if it doesn't exist
    if (!directAudio) {
        directAudio = document.createElement('audio');
        directAudio.autoplay = true;
        directAudio.controls = false;
        directAudio.style.display = 'none';
        document.body.appendChild(directAudio);
        
        // Handle events
        directAudio.addEventListener('playing', () => {
            console.log('Direct streaming started');
            showStatus('Connected to audio stream');
            startBtn.textContent = 'Disconnect';
            startBtn.disabled = false;
            startBtn.dataset.connected = 'true';
            isPlaying = true;
            
            // Clear timeout if any
            if (connectionTimeout) {
                clearTimeout(connectionTimeout);
                connectionTimeout = null;
            }
            
            // Reset the audioLastUpdateTime
            audioLastUpdateTime = Date.now();
        });
        
        directAudio.addEventListener('error', (e) => {
            console.error('Audio streaming error:', e);
            handleStreamError('Error connecting to audio stream');
        });
        
        directAudio.addEventListener('ended', () => {
            console.log('Stream ended');
            handleStreamEnd();
        });
    }
    
    // Add a unique timestamp parameter to prevent caching
    const timestamp = new Date().getTime();
    directAudio.src = `/direct-stream?t=${timestamp}`;
    
    // Set volume
    directAudio.volume = volumeControl.value;
    
    // Start playback
    directAudio.load();
    directAudio.play().catch(e => {
        console.error('Error starting playback:', e);
        handleStreamError('Failed to start playback. Please try again.');
    });
    
    // Set timeout for connection
    connectionTimeout = setTimeout(() => {
        console.log('Connection timeout');
        handleStreamError('Connection timeout');
    }, 10000); // 10 seconds timeout
    
    // Set up stall detection
    setupStallDetection();
}

// Set up stall detection to recover from audio stalls
function setupStallDetection() {
    if (window.stallDetectionInterval) {
        clearInterval(window.stallDetectionInterval);
    }
    
    window.stallDetectionInterval = setInterval(() => {
        // Check if audio is playing but stalled
        if (directAudio && !directAudio.paused && directAudio.readyState > 0) {
            // If no updates for 10 seconds, consider it stalled
            if (Date.now() - audioLastUpdateTime > 10000) {
                console.log('Audio appears to be stalled, restarting stream');
                handleStreamError('Audio stream stalled. Reconnecting...');
            }
        }
    }, 5000);
}

// Handle stream errors
function handleStreamError(message) {
    console.error(message);
    showStatus(message, true);
    
    // Clean up
    stopAudio(true);
    
    // Try to reconnect if appropriate
    if (reconnectAttempts < maxReconnectAttempts) {
        reconnectAttempts++;
        const delay = Math.min(1000 * Math.pow(2, reconnectAttempts - 1), 10000); // Exponential backoff
        
        console.log(`Reconnect attempt ${reconnectAttempts} in ${delay}ms`);
        showStatus(`Connection lost. Reconnecting in ${delay/1000}s...`, true);
        
        setTimeout(() => {
            if (startBtn.dataset.connected === 'true') {
                console.log('Attempting to reconnect...');
                startAudio();
            }
        }, delay);
    } else {
        console.log('Max reconnect attempts reached');
        showStatus('Could not connect to the server. Please try again later.', true);
        
        // Reset UI
        startBtn.textContent = 'Connect';
        startBtn.dataset.connected = 'false';
    }
}

// Handle stream end
function handleStreamEnd() {
    console.log('Stream ended');
    
    // Since our server handles track switching internally, 
    // an ended event likely means a problem occurred
    handleStreamError('Stream ended unexpectedly');
}

// Stop audio streaming
function stopAudio(isError = false) {
    console.log('Stopping audio playback');
    
    isPlaying = false;
    
    // Clear any intervals
    if (checkNowPlayingInterval) {
        clearInterval(checkNowPlayingInterval);
        checkNowPlayingInterval = null;
    }
    
    if (window.stallDetectionInterval) {
        clearInterval(window.stallDetectionInterval);
        window.stallDetectionInterval = null;
    }
    
    // Close WebSocket if open
    if (ws) {
        ws.close();
        ws = null;
    }
    
    // Clear media source
    if (mediaSource && mediaSource.readyState === 'open') {
        try {
            mediaSource.endOfStream();
        } catch (e) {
            console.error('Error ending media source stream:', e);
        }
    }
    
    // Clear queued data
    audioQueue = [];
    isProcessingQueue = false;
    
    // Clean up audio element
    if (directAudio) {
        directAudio.pause();
        directAudio.src = '';
        directAudio.load(); // Important: forces the element to reset
    }
    
    // Clear any pending timeout
    if (connectionTimeout) {
        clearTimeout(connectionTimeout);
        connectionTimeout = null;
    }
    
    if (!isError) {
        showStatus('Disconnected from audio stream');
    }
    
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

// Event listeners
startBtn.addEventListener('click', toggleConnection);

volumeControl.addEventListener('input', () => {
    if (directAudio) {
        directAudio.volume = volumeControl.value;
    }
    
    localStorage.setItem('radioVolume', volumeControl.value);
});

muteBtn.addEventListener('click', () => {
    if (directAudio) {
        directAudio.muted = !directAudio.muted;
        muteBtn.textContent = directAudio.muted ? 'Unmute' : 'Mute';
    }
});

// Handle page visibility
document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') {
        console.log('Page is now visible');
        
        // Update now playing
        updateNowPlaying();
        
        // Reconnect if needed and if the user was previously connected
        if (startBtn.dataset.connected === 'true' && (!directAudio || directAudio.paused || directAudio.ended)) {
            console.log('Reconnecting after page became visible');
            // Add a short delay to allow the browser to stabilize after becoming visible
            setTimeout(() => {
                startAudio();
            }, 500);
        }
    }
});

// Handle page reload/unload
window.addEventListener('beforeunload', () => {
    // Properly clean up resources
    if (directAudio) {
        directAudio.pause();
        directAudio.src = '';
    }
    
    if (ws) {
        ws.close();
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
    
    // Regular stats update
    setInterval(updateStats, 10000);
    
    console.log('Initialization complete');
});