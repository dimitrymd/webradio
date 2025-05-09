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

// Debug configuration
const DEBUG = true;
const DEBUG_AUDIO = true;
const DEBUG_WEBSOCKET = true;
const DEBUG_TRACK_INFO = true;

// Debug UI elements
let debugContainer = null;
let debugLog = null;

// Setup debug UI
function setupDebugUI() {
    // Create debug UI if in debug mode
    if (!DEBUG) return;
    
    console.log("Setting up debug UI");
    
    // Create debug container
    debugContainer = document.createElement('div');
    debugContainer.id = 'debug-container';
    debugContainer.style.cssText = `
        position: fixed;
        bottom: 10px;
        right: 10px;
        width: 600px;
        max-height: 400px;
        background: rgba(0, 0, 0, 0.8);
        color: #00ff00;
        font-family: monospace;
        font-size: 12px;
        padding: 10px;
        border-radius: 5px;
        z-index: 9999;
        overflow-y: auto;
    `;
    
    // Create debug header with controls
    const debugHeader = document.createElement('div');
    debugHeader.style.cssText = `
        display: flex;
        justify-content: space-between;
        margin-bottom: 5px;
        border-bottom: 1px solid #00ff00;
        padding-bottom: 5px;
    `;
    
    const debugTitle = document.createElement('span');
    debugTitle.textContent = '🔧 Radio Debug Console';
    
    const debugControls = document.createElement('div');
    
    // Clear button
    const clearBtn = document.createElement('button');
    clearBtn.textContent = 'Clear';
    clearBtn.style.cssText = `
        background: transparent;
        color: #00ff00;
        border: 1px solid #00ff00;
        margin-right: 5px;
        cursor: pointer;
    `;
    clearBtn.onclick = () => {
        debugLog.innerHTML = '';
        logDebug('Log cleared');
    };
    
    // Close button
    const closeBtn = document.createElement('button');
    closeBtn.textContent = 'Hide';
    closeBtn.style.cssText = `
        background: transparent;
        color: #00ff00;
        border: 1px solid #00ff00;
        cursor: pointer;
    `;
    closeBtn.onclick = () => {
        debugContainer.style.display = 'none';
        debugToggleBtn.style.display = 'block';
    };
    
    debugControls.appendChild(clearBtn);
    debugControls.appendChild(closeBtn);
    
    debugHeader.appendChild(debugTitle);
    debugHeader.appendChild(debugControls);
    
    // Create log area
    debugLog = document.createElement('div');
    debugLog.style.cssText = `
        overflow-y: auto;
        max-height: 350px;
    `;
    
    // Add tabs for different debug sections
    const tabsContainer = document.createElement('div');
    tabsContainer.style.cssText = `
        display: flex;
        margin-top: 5px;
        margin-bottom: 5px;
    `;
    
    const tabs = [
        { id: 'all', label: 'All' },
        { id: 'audio', label: 'Audio' },
        { id: 'ws', label: 'WebSocket' },
        { id: 'track', label: 'Track Info' }
    ];
    
    tabs.forEach(tab => {
        const tabElement = document.createElement('div');
        tabElement.textContent = tab.label;
        tabElement.dataset.tab = tab.id;
        tabElement.className = tab.id === 'all' ? 'debug-tab active' : 'debug-tab';
        tabElement.style.cssText = `
            padding: 3px 8px;
            margin-right: 5px;
            cursor: pointer;
            border: 1px solid #00ff00;
            border-radius: 3px;
        `;
        
        if (tab.id === 'all') {
            tabElement.style.background = '#005500';
        }
        
        tabElement.onclick = () => {
            // Remove active class from all tabs
            document.querySelectorAll('.debug-tab').forEach(t => {
                t.classList.remove('active');
                t.style.background = 'transparent';
            });
            
            // Add active class to clicked tab
            tabElement.classList.add('active');
            tabElement.style.background = '#005500';
            
            // Filter log entries
            if (tab.id === 'all') {
                document.querySelectorAll('.debug-entry').forEach(entry => {
                    entry.style.display = 'block';
                });
            } else {
                document.querySelectorAll('.debug-entry').forEach(entry => {
                    if (entry.classList.contains(`type-${tab.id}`)) {
                        entry.style.display = 'block';
                    } else {
                        entry.style.display = 'none';
                    }
                });
            }
        };
        
        tabsContainer.appendChild(tabElement);
    });
    
    // Add test buttons for common actions
    const testContainer = document.createElement('div');
    testContainer.style.cssText = `
        display: flex;
        flex-wrap: wrap;
        gap: 5px;
        margin-top: 5px;
        margin-bottom: 5px;
    `;
    
    const testButtons = [
        { label: 'Test Connection', action: testConnection },
        { label: 'Check Now Playing', action: checkNowPlaying },
        { label: 'Restart Stream', action: restartStream },
        { label: 'Browser Support', action: checkBrowserSupport }
    ];
    
    testButtons.forEach(button => {
        const btnElement = document.createElement('button');
        btnElement.textContent = button.label;
        btnElement.style.cssText = `
            background: transparent;
            color: #00ff00;
            border: 1px solid #00ff00;
            padding: 3px 6px;
            font-size: 11px;
            cursor: pointer;
        `;
        btnElement.onclick = button.action;
        testContainer.appendChild(btnElement);
    });
    
    // Assemble everything
    debugContainer.appendChild(debugHeader);
    debugContainer.appendChild(tabsContainer);
    debugContainer.appendChild(testContainer);
    debugContainer.appendChild(debugLog);
    
    // Create toggle button
    const debugToggleBtn = document.createElement('button');
    debugToggleBtn.id = 'debug-toggle';
    debugToggleBtn.textContent = '🔧 Debug';
    debugToggleBtn.style.cssText = `
        position: fixed;
        bottom: 10px;
        right: 10px;
        background: rgba(0, 0, 0, 0.8);
        color: #00ff00;
        border: 1px solid #00ff00;
        border-radius: 5px;
        padding: 5px 10px;
        font-family: monospace;
        cursor: pointer;
        z-index: 10000;
    `;
    debugToggleBtn.onclick = () => {
        debugContainer.style.display = 'block';
        debugToggleBtn.style.display = 'none';
    };
    
    // Add to DOM
    document.body.appendChild(debugContainer);
    document.body.appendChild(debugToggleBtn);
    
    // Log initial debug info
    logDebug('Debug mode activated');
    logDebug(`User agent: ${navigator.userAgent}`);
    
    // Add CSS
    const style = document.createElement('style');
    style.textContent = `
        .debug-entry {
            margin-bottom: 3px;
            border-bottom: 1px solid #333;
            padding-bottom: 3px;
            font-family: monospace;
            word-break: break-word;
        }
        .debug-tab.active {
            background: #005500;
        }
        .debug-entry.type-audio {
            color: #66aaff;
        }
        .debug-entry.type-ws {
            color: #ffaa66;
        }
        .debug-entry.type-track {
            color: #66ffaa;
        }
        .debug-entry.error {
            color: #ff6666 !important;
        }
    `;
    document.head.appendChild(style);
}

// Log to debug console
function logDebug(message, type = 'general', isError = false) {
    if (!DEBUG) return;
    
    // Skip certain high-volume logging if specific debug flags are off
    if (type === 'audio' && !DEBUG_AUDIO) return;
    if (type === 'ws' && !DEBUG_WEBSOCKET) return;
    if (type === 'track' && !DEBUG_TRACK_INFO) return;
    
    const timestamp = new Date().toISOString().substring(11, 23);
    
    // Log to browser console
    if (isError) {
        console.error(`[${timestamp}] [${type}] ${message}`);
    } else {
        console.log(`[${timestamp}] [${type}] ${message}`);
    }
    
    // Log to debug UI if it exists
    if (debugLog) {
        const entry = document.createElement('div');
        entry.className = `debug-entry type-${type}${isError ? ' error' : ''}`;
        entry.innerHTML = `<span class="timestamp">[${timestamp}]</span> ${message}`;
        
        debugLog.insertBefore(entry, debugLog.firstChild);
        
        // Limit entries to prevent browser slowdown
        if (debugLog.children.length > 1000) {
            debugLog.removeChild(debugLog.lastChild);
        }
    }
}

// Test connection to the server
async function testConnection() {
    logDebug('Testing connection to server...', 'general');
    
    try {
        const startTime = performance.now();
        const response = await fetch('/api/stats');
        const endTime = performance.now();
        const latency = Math.round(endTime - startTime);
        
        if (response.ok) {
            const data = await response.json();
            logDebug(`Connection successful! Latency: ${latency}ms`, 'general');
            logDebug(`Stats: ${JSON.stringify(data)}`, 'general');
        } else {
            logDebug(`Server returned error status: ${response.status}`, 'general', true);
        }
    } catch (error) {
        logDebug(`Connection test failed: ${error.message}`, 'general', true);
    }
}

// Check now playing info
async function checkNowPlaying() {
    logDebug('Checking now playing info...', 'track');
    
    try {
        const response = await fetch('/api/now-playing');
        
        if (response.ok) {
            const data = await response.json();
            logDebug(`Now playing: ${JSON.stringify(data)}`, 'track');
            
            // Check if we actually have a track
            if (data.error) {
                logDebug(`API returned error: ${data.error}`, 'track', true);
            } else {
                logDebug(`Track: "${data.title}" by "${data.artist}"`, 'track');
                if (data.playback_position !== undefined) {
                    logDebug(`Position: ${data.playback_position}s / ${data.duration}s`, 'track');
                }
            }
        } else {
            logDebug(`Server returned error status: ${response.status}`, 'track', true);
        }
    } catch (error) {
        logDebug(`Now playing check failed: ${error.message}`, 'track', true);
    }
}

// Restart the audio stream
function restartStream() {
    logDebug('Manually restarting stream...', 'audio');
    
    // First stop any current stream
    stopAudio();
    
    // Wait a moment then start again
    setTimeout(() => {
        logDebug('Starting new stream connection', 'audio');
        startAudio();
    }, 1000);
}

// Check browser support for streaming technologies
function checkBrowserSupport() {
    logDebug('Checking browser support for streaming...', 'general');
    
    // Check MediaSource support
    if ('MediaSource' in window) {
        logDebug('✓ MediaSource API is supported', 'general');
        
        // Check various formats
        const formats = [
            'audio/mpeg',
            'audio/aac',
            'audio/webm',
            'audio/ogg'
        ];
        
        formats.forEach(format => {
            const supported = MediaSource.isTypeSupported(format);
            logDebug(`${supported ? '✓' : '✗'} Format ${format} ${supported ? 'is' : 'is NOT'} supported`, 'general');
        });
    } else {
        logDebug('✗ MediaSource API is NOT supported - will use fallback streaming', 'general', true);
    }
    
    // Check WebSocket support
    if ('WebSocket' in window) {
        logDebug('✓ WebSocket API is supported', 'general');
    } else {
        logDebug('✗ WebSocket API is NOT supported', 'general', true);
    }
    
    // Check AudioContext support
    if ('AudioContext' in window || 'webkitAudioContext' in window) {
        logDebug('✓ AudioContext API is supported', 'general');
    } else {
        logDebug('✗ AudioContext API is NOT supported', 'general', true);
    }
    
    // Check auto-play policy
    logDebug('Testing autoplay capability...', 'general');
    
    const audioTest = document.createElement('audio');
    audioTest.src = 'data:audio/wav;base64,UklGRiQAAABXQVZFZm10IBAAAAABAAEARKwAAIhYAQACABAAZGF0YQAAAAA='; // Tiny silent audio
    
    audioTest.play()
        .then(() => {
            logDebug('✓ Autoplay is allowed', 'general');
        })
        .catch(error => {
            logDebug(`✗ Autoplay is blocked: ${error.message}`, 'general', true);
            logDebug('User interaction will be required to play audio', 'general');
        })
        .finally(() => {
            audioTest.remove();
        });
    
    // Log current connection info
    logDebug(`Current protocol: ${window.location.protocol}`, 'general');
    logDebug(`Current host: ${window.location.host}`, 'general');
    
    // Log network information if available
    if ('connection' in navigator) {
        const conn = navigator.connection;
        logDebug(`Network type: ${conn.effectiveType || 'unknown'}`, 'general');
        logDebug(`Downlink: ${conn.downlink || 'unknown'} Mbps`, 'general');
    }
}

// Helper function to get audio error messages
function getAudioErrorMessage(errorCode) {
    switch(errorCode) {
        case 1:
            return "MEDIA_ERR_ABORTED - Fetching process aborted by user";
        case 2: 
            return "MEDIA_ERR_NETWORK - Network error while loading media";
        case 3:
            return "MEDIA_ERR_DECODE - Media decoding error (corrupt or unsupported format)";
        case 4:
            return "MEDIA_ERR_SRC_NOT_SUPPORTED - Media format not supported";
        default:
            return "Unknown error";
    }
}

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
    logDebug(`Status message: ${message}${isError ? ' (ERROR)' : ''}`, 'general', isError);
    
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
        logDebug("Fetching now playing info...", 'track');
        const response = await fetch('/api/now-playing');
        if (!response.ok) {
            logDebug(`Failed to fetch now playing info: ${response.status}`, 'track', true);
            throw new Error(`Failed to fetch now playing info: ${response.status}`);
        }
        
        const data = await response.json();
        logDebug(`Received now playing info: ${JSON.stringify(data)}`, 'track');
        
        if (data.error) {
            currentTitle.textContent = 'No tracks available';
            currentArtist.textContent = 'Please add MP3 files to the server';
            currentAlbum.textContent = '';
            currentDuration.textContent = '';
            currentPosition.textContent = '';
            logDebug(`Now playing error: ${data.error}`, 'track', true);
        } else {
            // Store track ID (path) for change detection
            const newTrackId = data.path;
            const trackChanged = currentTitle.dataset.trackId !== newTrackId;
            
            if (trackChanged) {
                logDebug(`Track changed to: "${data.title}" by "${data.artist}"`, 'track');
                
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
                logDebug(`Playback position: ${data.playback_position}s / ${data.duration}s`, 'track');
            }
            
            // Update listener count if available
            if (data.active_listeners !== undefined) {
                listenerCount.textContent = `Listeners: ${data.active_listeners}`;
                logDebug(`Active listeners: ${data.active_listeners}`, 'track');
            }
            
            // Update page title
            document.title = `${data.title} - ${data.artist} | Rust Web Radio`;
            
            // Update the last update time
            audioLastUpdateTime = Date.now();
        }
    } catch (error) {
        logDebug(`Error fetching now playing: ${error.message}`, 'track', true);
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
        logDebug(`Updated stats: ${JSON.stringify(data)}`, 'general');
    } catch (error) {
        logDebug(`Error fetching stats: ${error.message}`, 'general', true);
    }
}

// Start audio streaming
function startAudio() {
    logDebug('Starting audio playback - user initiated', 'audio');
    startBtn.disabled = true;
    
    // Reset reconnect attempts
    reconnectAttempts = 0;
    
    // Clean up any existing audio elements
    if (directAudio) {
        logDebug('Cleaning up existing audio element', 'audio');
        directAudio.pause();
        directAudio.src = '';
        directAudio.load();
        if (directAudio.parentNode) {
            directAudio.parentNode.removeChild(directAudio);
        }
        directAudio = null;
    }
    
    // Determine best streaming method based on browser capabilities
    const mediaSourceSupported = 'MediaSource' in window && MediaSource.isTypeSupported('audio/mpeg');
    logDebug(`Streaming method: ${mediaSourceSupported ? 'MediaSource API' : 'Direct HTTP streaming'}`, 'audio');
    
    if (mediaSourceSupported) {
        logDebug('Using MediaSource API for streaming', 'audio');
        startMSEStreaming();
    } else {
        logDebug('Using direct HTTP streaming', 'audio');
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
    logDebug('Starting MSE streaming setup', 'audio');
    
    // Create a new audio element
    logDebug('Creating new audio element for MSE', 'audio');
    directAudio = document.createElement('audio');
    directAudio.autoplay = true;  // Safe now since user has interacted
    directAudio.controls = false;
    directAudio.style.display = 'none';
    document.body.appendChild(directAudio);
    
    // Set initial volume
    directAudio.volume = volumeControl.value;
    
    // Add event listeners for debugging
    directAudio.addEventListener('playing', () => {
        logDebug('Audio element started playing', 'audio');
    });
    
    directAudio.addEventListener('waiting', () => {
        logDebug('Audio buffering - waiting for more data', 'audio');
    });
    
    directAudio.addEventListener('stalled', () => {
        logDebug('Audio playback stalled', 'audio', true);
    });
    
    // Create Media Source
    logDebug('Creating MediaSource object', 'audio');
    mediaSource = new MediaSource();
    directAudio.src = URL.createObjectURL(mediaSource);
    
    // Handle Media Source open event
    mediaSource.addEventListener('sourceopen', function() {
        logDebug(`MediaSource opened, readyState: ${mediaSource.readyState}`, 'audio');
        
        // Create source buffer
        try {
            logDebug('Adding SourceBuffer for audio/mpeg', 'audio');
            sourceBuffer = mediaSource.addSourceBuffer('audio/mpeg');
            
            // Handle update end events
            sourceBuffer.addEventListener('updateend', function() {
                // Process the next item in the queue if available
                processQueue();
            });
            
            // Handle errors
            sourceBuffer.addEventListener('error', function(e) {
                logDebug(`SourceBuffer error: ${e.message || 'Unknown error'}`, 'audio', true);
            });
            
            // Connect to WebSocket
            connectWebSocket();
        } catch (e) {
            logDebug(`Error setting up MSE: ${e.message}`, 'audio', true);
            logDebug(`Stack trace: ${e.stack}`, 'audio', true);
            // Fall back to direct streaming if MSE fails
            startDirectStreaming();
        }
    });
    
    mediaSource.addEventListener('sourceclose', function() {
        logDebug('MediaSource closed', 'audio');
    });
    
    mediaSource.addEventListener('sourceended', function() {
        logDebug('MediaSource ended', 'audio');
    });
    
    mediaSource.addEventListener('error', function(e) {
        logDebug(`MediaSource error: ${e.message || 'Unknown error'}`, 'audio', true);
    });
    
    // Set up timeout for initial connection
    connectionTimeout = setTimeout(function() {
        logDebug('Connection timeout for MSE - falling back to direct streaming', 'audio', true);
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
            
            // Log progress occasionally
            if (audioQueue.length % 50 === 0 && audioQueue.length > 0) {
                logDebug(`Queue status: ${audioQueue.length} chunks pending`, 'audio');
            }
        } catch (e) {
            logDebug(`Error appending buffer: ${e.name} - ${e.message}`, 'audio', true);
            
            // If we hit a quota exceeded error, clear the buffer and try again
            if (e.name === 'QuotaExceededError') {
                logDebug('Buffer full, removing old data', 'audio');
                // Remove 10 seconds from the beginning
                if (sourceBuffer.buffered.length > 0) {
                    const start = sourceBuffer.buffered.start(0);
                    const end = start + 10;
                    logDebug(`Removing buffer from ${start}s to ${end}s`, 'audio');
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
        logDebug('Closing existing WebSocket connection', 'ws');
        ws.close();
        ws = null;
    }
    
    // Determine the WebSocket URL
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/stream`;
    logDebug(`Connecting to WebSocket: ${wsUrl}`, 'ws');
    
    // Create new WebSocket
    ws = new WebSocket(wsUrl);
    
    // Set up event handlers
    ws.onopen = function() {
        logDebug('WebSocket connection established', 'ws');
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
            // Log bin message size occasionally
            if (Math.random() < 0.01) { // Log roughly 1% of binary messages
                logDebug(`Received binary data: ${event.data.size} bytes`, 'ws');
            }
            
            // Handle binary audio data
            event.data.arrayBuffer().then(buffer => {
                if (sourceBuffer && mediaSource.readyState === 'open') {
                    // Add to queue
                    audioQueue.push(buffer);
                    
                    // Process queue if not already processing
                    if (!isProcessingQueue && !sourceBuffer.updating) {
                        processQueue();
                    }
                } else {
                    if (mediaSource) {
                        logDebug(`Cannot process audio chunk - MediaSource state: ${mediaSource.readyState}`, 'audio', true);
                    } else {
                        logDebug('Cannot process audio chunk - MediaSource not available', 'audio', true);
                    }
                }
            }).catch(e => {
                logDebug(`Error processing audio data: ${e.message}`, 'audio', true);
            });
        } else {
            // Handle text data (track info)
            try {
                logDebug(`Received text message: ${event.data}`, 'ws');
                const info = JSON.parse(event.data);
                logDebug(`Parsed track info: ${JSON.stringify(info)}`, 'track');
                
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
                logDebug(`Error parsing track info: ${e.message}`, 'track', true);
            }
        }
    };
    
    ws.onclose = function(event) {
        logDebug(`WebSocket connection closed: Code ${event.code}`, 'ws');
        
        // Only attempt reconnect if it wasn't requested by the user
        if (startBtn.dataset.connected === 'true' && isPlaying) {
            handleStreamError('Connection lost. Attempting to reconnect...');
        }
    };
    
    ws.onerror = function(error) {
        logDebug('WebSocket error occurred', 'ws', true);
        handleStreamError('Error connecting to audio stream');
    };
}

// Direct HTTP streaming
function startDirectStreaming() {
    logDebug('Starting direct HTTP streaming', 'audio');
    
    // Create a new audio element
    logDebug('Creating new audio element for direct streaming', 'audio');
    directAudio = document.createElement('audio');
    directAudio.autoplay = true;  // Safe now since user has interacted
    directAudio.controls = false;
    directAudio.style.display = 'none';
    document.body.appendChild(directAudio);
    
    // Handle events
    directAudio.addEventListener('playing', () => {
        logDebug('Direct streaming started successfully', 'audio');
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
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        const errorMessage = getAudioErrorMessage(errorCode);
        logDebug(`Audio streaming error (code ${errorCode}): ${errorMessage}`, 'audio', true);
        handleStreamError(`Error connecting to audio stream: ${errorMessage}`);
    });
    
    directAudio.addEventListener('ended', () => {
        logDebug('Stream ended', 'audio', true);
        handleStreamEnd();
    });
    
    // Add stalled and waiting events
    directAudio.addEventListener('stalled', () => {
        logDebug('Audio playback stalled', 'audio', true);
    });
    
    directAudio.addEventListener('waiting', () => {
        logDebug('Audio playback waiting for more data', 'audio');
    });
    
    // Add canplay event
    directAudio.addEventListener('canplay', () => {
        logDebug('Audio can play - enough data is available', 'audio');
    });
    
    // Add timeupdate event to monitor playback
    directAudio.addEventListener('timeupdate', () => {
        if (directAudio.currentTime % 10 < 0.1) { // Log every ~10 seconds
            logDebug(`Audio playback time: ${Math.floor(directAudio.currentTime)}s`, 'audio');
        }
    });
    
    // Add a unique timestamp parameter to prevent caching
    const timestamp = new Date().getTime();
    const streamUrl = `/direct-stream?t=${timestamp}`;
    logDebug(`Setting audio source to: ${streamUrl}`, 'audio');
    directAudio.src = streamUrl;
    
    // Set volume
    directAudio.volume = volumeControl.value;
    
    // Start playback
    logDebug('Attempting to start audio playback', 'audio');
    directAudio.load();
    directAudio.play().catch(e => {
        logDebug(`Error starting playback: ${e.message}`, 'audio', true);
        handleStreamError('Failed to start playback. Please try again.');
    });
    
    // Set timeout for connection
    logDebug('Setting 10-second connection timeout', 'audio');
    connectionTimeout = setTimeout(() => {
        logDebug('Connection timeout for direct streaming', 'audio', true);
        handleStreamError('Connection timeout');
    }, 10000); // 10 seconds timeout
    
    // Set up stall detection
    setupStallDetection();
}

// Set up stall detection to recover from audio stalls
function setupStallDetection() {
    logDebug('Setting up stall detection', 'audio');
    
    if (window.stallDetectionInterval) {
        clearInterval(window.stallDetectionInterval);
    }
    
    let lastPlaybackTime = 0;
    let stallCounter = 0;
    
    window.stallDetectionInterval = setInterval(() => {
        // Check if audio is playing but stalled
        if (directAudio && !directAudio.paused && directAudio.readyState > 0) {
            // If current time hasn't advanced in 5 seconds, it might be stalled
            if (directAudio.currentTime === lastPlaybackTime) {
                stallCounter++;
                
                if (stallCounter >= 3) { // ~15 seconds of no progress
                    logDebug('Stream appears to be stalled, attempting recovery', 'audio', true);
                    
                    // Attempt recovery: restart the stream
                    handleStreamError('Audio stream stalled. Reconnecting...');
                    stallCounter = 0;
                } else {
                    logDebug(`Possible stall detected (${stallCounter}/3): Playback time not advancing`, 'audio');
                }
            } else {
                // Reset counter if playback is advancing
                if (stallCounter > 0) {
                    logDebug('Playback time advancing again, stall resolved', 'audio');
                    stallCounter = 0;
                }
            }
            
            lastPlaybackTime = directAudio.currentTime;
        }
        
        // Also check for too much time since last data
        if (Date.now() - audioLastUpdateTime > 10000 && isPlaying) {
            logDebug('No audio data received for 10+ seconds, possible network issue', 'audio', true);
            stallCounter++;
            
            if (stallCounter >= 3) {
                logDebug('Long period with no data, attempting recovery', 'audio', true);
                handleStreamError('No data received. Reconnecting...');
                stallCounter = 0;
            }
        }
    }, 5000);
}

// Handle stream errors
function handleStreamError(message) {
    logDebug(`Stream error: ${message}`, 'audio', true);
    showStatus(message, true);
    
    // Clean up
    stopAudio(true);
    
    // Try to reconnect if appropriate
    if (reconnectAttempts < maxReconnectAttempts) {
        reconnectAttempts++;
        const delay = Math.min(1000 * Math.pow(2, reconnectAttempts - 1), 10000); // Exponential backoff
        
        logDebug(`Reconnect attempt ${reconnectAttempts} in ${delay}ms`, 'audio');
        showStatus(`Connection lost. Reconnecting in ${delay/1000}s...`, true);
        
        setTimeout(() => {
            if (startBtn.dataset.connected === 'true') {
                logDebug('Attempting to reconnect...', 'audio');
                startAudio();
            }
        }, delay);
    } else {
        logDebug('Max reconnect attempts reached', 'audio', true);
        showStatus('Could not connect to the server. Please try again later.', true);
        
        // Reset UI
        startBtn.textContent = 'Connect';
        startBtn.disabled = false;
        startBtn.dataset.connected = 'false';
    }
}

// Handle stream end
function handleStreamEnd() {
    logDebug('Stream ended', 'audio');
    
    // Since our server handles track switching internally, 
    // an ended event likely means a problem occurred
    handleStreamError('Stream ended unexpectedly');
}

// Stop audio streaming
function stopAudio(isError = false) {
    logDebug(`Stopping audio playback${isError ? ' due to error' : ' by user request'}`, 'audio');
    
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
            logDebug(`Error ending media source stream: ${e.message}`, 'audio', true);
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
        
        // Remove from DOM
        if (directAudio.parentNode) {
            directAudio.parentNode.removeChild(directAudio);
        }
        directAudio = null;
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
        logDebug('User requested disconnect', 'general');
        stopAudio();
    } else {
        logDebug('User requested connect - starting audio now', 'general');
        startAudio();  // This creates and starts the audio only after button click
    }
}

// Monitor WebSocket state and report issues
function monitorWebSocketHealth() {
    if (!ws) {
        logDebug('No active WebSocket connection to monitor', 'ws');
        return;
    }
    
    const states = ['CONNECTING', 'OPEN', 'CLOSING', 'CLOSED'];
    logDebug(`WebSocket state: ${states[ws.readyState]} (${ws.readyState})`, 'ws');
    
    if (ws.readyState === WebSocket.OPEN) {
        logDebug('WebSocket connection is healthy', 'ws');
        
        // Send a ping to verify connection
        try {
            // Use a timestamp as ping data
            const pingData = new Date().getTime().toString();
            ws.send(pingData);
            logDebug('WebSocket ping sent', 'ws');
        } catch (e) {
            logDebug(`WebSocket ping failed: ${e.message}`, 'ws', true);
        }
    } else if (ws.readyState === WebSocket.CLOSED || ws.readyState === WebSocket.CLOSING) {
        logDebug('WebSocket connection is closed or closing', 'ws', true);
    }
}

// Event listeners
startBtn.addEventListener('click', toggleConnection);

volumeControl.addEventListener('input', () => {
    if (directAudio) {
        directAudio.volume = volumeControl.value;
        logDebug(`Volume set to ${volumeControl.value}`, 'audio');
    }
    
    localStorage.setItem('radioVolume', volumeControl.value);
});

muteBtn.addEventListener('click', () => {
    if (directAudio) {
        directAudio.muted = !directAudio.muted;
        muteBtn.textContent = directAudio.muted ? 'Unmute' : 'Mute';
        logDebug(`Audio ${directAudio.muted ? 'muted' : 'unmuted'}`, 'audio');
    }
});

// Handle page visibility
document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') {
        logDebug('Page is now visible', 'general');
        
        // Update now playing
        updateNowPlaying();
        
        // Reconnect if needed and if the user was previously connected
        if (startBtn.dataset.connected === 'true' && (!directAudio || directAudio.paused || directAudio.ended)) {
            logDebug('Reconnecting after page became visible', 'audio');
            // Add a short delay to allow the browser to stabilize after becoming visible
            setTimeout(() => {
                startAudio();
            }, 500);
        }
    } else {
        logDebug('Page is now hidden', 'general');
    }
});

// Handle page reload/unload
window.addEventListener('beforeunload', () => {
    // Properly clean up resources
    logDebug('Page unloading, cleaning up resources', 'general');
    
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
    
    // Set up debug UI
    setupDebugUI();
    
    // Run browser checks
    checkBrowserSupport();
    
    // Set initial button state - IMPORTANT: start in 'Connect' state
    startBtn.textContent = 'Connect';
    startBtn.dataset.connected = 'false';
    
    // Set initial volume (but don't create audio elements yet)
    const savedVolume = localStorage.getItem('radioVolume');
    if (savedVolume !== null) {
        volumeControl.value = savedVolume;
        logDebug(`Restored saved volume: ${savedVolume}`, 'audio');
    }
    
    // Update now playing display but don't start audio
    await updateNowPlaying();
    
    // Regular stats update
    setInterval(updateStats, 10000);
    
    logDebug('Initialization complete - waiting for user to click Connect', 'general');
});