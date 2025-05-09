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
const progressBar = document.getElementById('progress-bar');

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
        logDebug('✗ MediaSource API is NOT supported', 'general', true);
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

// Start WebSocket streaming
function startAudio() {
    logDebug('Starting audio playback - user initiated', 'audio');
    startBtn.disabled = true;
    
    // Reset reconnect attempts
    reconnectAttempts = 0;
    
    // Check if WebSocket API is supported
    if (!('WebSocket' in window)) {
        logDebug('WebSocket API not supported by this browser', 'audio', true);
        showStatus('Your browser does not support WebSockets. Please try a different browser.', true);
        startBtn.disabled = false;
        return;
    }
    
    // Check if MediaSource API is supported
    if (!('MediaSource' in window) || !MediaSource.isTypeSupported('audio/mpeg')) {
        logDebug('MediaSource API not supported for MP3 by this browser', 'audio', true);
        showStatus('Your browser does not fully support MediaSource for MP3. Audio may not play correctly.', true);
        // We'll still try to connect, but warn the user
    }
    
    logDebug('Connecting to WebSocket stream...', 'audio');
    connectWebSocket();
    
    // Start frequent checks of now playing info
    if (checkNowPlayingInterval) {
        clearInterval(checkNowPlayingInterval);
    }
    checkNowPlayingInterval = setInterval(updateNowPlaying, 2000);
}

// Connect to WebSocket for streaming
function connectWebSocket() {
    // Clean up any existing WebSocket
    if (ws) {
        logDebug('Closing existing WebSocket connection', 'ws');
        ws.close();
        ws = null;
    }
    
    // Clean up any existing MediaSource
    if (mediaSource) {
        if (mediaSource.readyState === 'open') {
            try {
                mediaSource.endOfStream();
            } catch (e) {
                logDebug(`Error ending MediaSource stream: ${e.message}`, 'audio', true);
            }
        }
        mediaSource = null;
    }
    
    // Clean up any existing audio element
    if (audioContext) {
        try {
            audioContext.close();
        } catch (e) {
            logDebug(`Error closing AudioContext: ${e.message}`, 'audio', true);
        }
        audioContext = null;
    }
    
    // Create audio context
    try {
        audioContext = new (window.AudioContext || window.webkitAudioContext)();
        logDebug(`Created AudioContext, sample rate: ${audioContext.sampleRate}Hz`, 'audio');
    } catch (e) {
        logDebug(`Error creating AudioContext: ${e.message}`, 'audio', true);
        showStatus('Error initializing audio. Please try again.', true);
        startBtn.disabled = false;
        return;
    }
    
    // Create a new MediaSource
    try {
        mediaSource = new MediaSource();
        logDebug(`Created MediaSource object, readyState: ${mediaSource.readyState}`, 'audio');
    } catch (e) {
        logDebug(`Error creating MediaSource: ${e.message}`, 'audio', true);
        showStatus('Error initializing media. Please try again.', true);
        startBtn.disabled = false;
        return;
    }
    
    // Set up MediaSource open handler
    mediaSource.addEventListener('sourceopen', function() {
        logDebug(`MediaSource opened, readyState: ${mediaSource.readyState}`, 'audio');
        
        try {
            // Create source buffer
            sourceBuffer = mediaSource.addSourceBuffer('audio/mpeg');
            logDebug('SourceBuffer created for audio/mpeg', 'audio');
            
            sourceBuffer.addEventListener('updateend', function() {
                // Process the queue when source buffer is ready
                processQueue();
            });
            
            sourceBuffer.addEventListener('error', function(e) {
                logDebug(`SourceBuffer error: ${e.message || 'Unknown error'}`, 'audio', true);
            });
            
            // Reset audio queue
            audioQueue = [];
            isProcessingQueue = false;
            
            // Determine the WebSocket URL
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            const wsUrl = `${protocol}//${window.location.host}/stream`;
            logDebug(`Connecting to WebSocket at ${wsUrl}`, 'ws');
            
            // Create WebSocket connection
            ws = new WebSocket(wsUrl);
            
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
            
            ws.onmessage = handleWebSocketMessage;
        } catch (e) {
            logDebug(`Error setting up streaming: ${e.message}`, 'audio', true);
            showStatus(`Error setting up stream: ${e.message}`, true);
            startBtn.disabled = false;
        }
    });
    
    mediaSource.addEventListener('sourceclose', function() {
        logDebug('MediaSource closed', 'audio');
    });
    
    mediaSource.addEventListener('sourceended', function() {
        logDebug('MediaSource ended', 'audio');
    });
    
    // Create audio element and connect it to the media source
    const audioElement = document.createElement('audio');
    audioElement.id = 'audio-stream';
    audioElement.style.display = 'none';
    document.body.appendChild(audioElement);
    
    // Set volume
    audioElement.volume = volumeControl.value;
    
    // Create object URL from media source
    const mediaSourceUrl = URL.createObjectURL(mediaSource);
    audioElement.src = mediaSourceUrl;
    
    // Add event listeners
    audioElement.addEventListener('playing', function() {
        logDebug('Audio playback started', 'audio');
    });
    
    audioElement.addEventListener('waiting', function() {
        logDebug('Audio buffering - waiting for more data', 'audio');
    });
    
    audioElement.addEventListener('stalled', function() {
        logDebug('Audio playback stalled', 'audio', true);
    });
    
    audioElement.addEventListener('error', function(e) {
        const errorCode = e.target.error ? e.target.error.code : 'unknown';
        logDebug(`Audio error (code ${errorCode})`, 'audio', true);
    });
    
    // Start playback
    audioElement.play().catch(function(e) {
        logDebug(`Error starting playback: ${e.message}`, 'audio', true);
        showStatus('Error starting playback. Please try again.', true);
        startBtn.disabled = false;
    });
    
    // Set timeout for initial connection
    connectionTimeout = setTimeout(function() {
        logDebug('Connection timeout - no audio data received', 'audio', true);
        handleStreamError('Connection timeout. Please try again.');
    }, 10000);
}

// Handle WebSocket messages
function handleWebSocketMessage(event) {
    // Clear connection timeout if set
    if (connectionTimeout) {
        clearTimeout(connectionTimeout);
        connectionTimeout = null;
    }
    
    // Reset the audioLastUpdateTime
    audioLastUpdateTime = Date.now();
    
    // Process binary audio data
    if (event.data instanceof Blob) {
        // Log bin message size occasionally
        if (Math.random() < 0.01) { // Log roughly 1% of binary messages
            logDebug(`Received binary data: ${event.data.size} bytes`, 'ws');
        }
        
        // Convert blob to array buffer
        event.data.arrayBuffer().then(buffer => {
            if (sourceBuffer && mediaSource && mediaSource.readyState === 'open') {
                // Add to queue
                audioQueue.push(buffer);
                
                // Process queue if not already processing
                if (!isProcessingQueue && !sourceBuffer.updating) {
                    processQueue();
                }
            } else {
                logDebug('Cannot process audio data - MediaSource not ready', 'audio');
            }
        }).catch(e => {
            logDebug(`Error processing audio data: ${e.message}`, 'audio', true);
        });
    } else {
        // Process text data (likely track info)
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
            logDebug(`Error parsing text message: ${event.data}`, 'track', true);
        }
    }
}

// Process the audio queue
function processQueue() {
    if (audioQueue.length > 0 && !isProcessingQueue && sourceBuffer && !sourceBuffer.updating) {
        isProcessingQueue = true;
        const data = audioQueue.shift();
        
        try {
            sourceBuffer.appendBuffer(data);
            
            // Log queue status periodically
            if (audioQueue.length % 50 === 0 && audioQueue.length > 0) {
                logDebug(`Queue status: ${audioQueue.length} chunks pending`, 'audio');
            }
        } catch (e) {
            logDebug(`Error appending buffer: ${e.name} - ${e.message}`, 'audio', true);
            
            // If we hit a quota exceeded error, clear part of the buffer
            if (e.name === 'QuotaExceededError') {
                logDebug('Buffer full, removing old data', 'audio');
                
                if (sourceBuffer.buffered.length > 0) {
                    try {
                        // Remove first 10 seconds of buffered data
                        const start = sourceBuffer.buffered.start(0);
                        const end = Math.min(sourceBuffer.buffered.end(0), start + 10);
                        sourceBuffer.remove(start, end);
                    } catch (removeError) {
                        logDebug(`Error removing buffer: ${removeError.message}`, 'audio', true);
                    }
                }
                
                // Put the data back in the queue
                audioQueue.unshift(data);
            }
        }
        
        isProcessingQueue = false;
        
        // Continue processing if there are more items and the buffer is not updating
        if (audioQueue.length > 0 && !sourceBuffer.updating) {
            processQueue();
        }
    }
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

// Stop audio streaming
function stopAudio(isError = false) {
    logDebug(`Stopping audio playback${isError ? ' due to error' : ' by user request'}`, 'audio');
    
    isPlaying = false;
    
    // Clear any intervals
    if (checkNowPlayingInterval) {
        clearInterval(checkNowPlayingInterval);
        checkNowPlayingInterval = null;
    }
    
    // Close WebSocket if open
    if (ws) {
        ws.close();
        ws = null;
    }
    
    // Clean up media source
    if (mediaSource && mediaSource.readyState === 'open') {
        try {
            mediaSource.endOfStream();
        } catch (e) {
            logDebug(`Error ending media source stream: ${e.message}`, 'audio', true);
        }
    }
    
    // Clear audio context
    if (audioContext) {
        try {
            audioContext.close();
        } catch (e) {
            logDebug(`Error closing audio context: ${e.message}`, 'audio', true);
        }
        audioContext = null;
    }
    
    // Clear queued data
    audioQueue = [];
    isProcessingQueue = false;
    
    // Remove audio element
    const audioElement = document.getElementById('audio-stream');
    if (audioElement) {
        audioElement.pause();
        audioElement.src = '';
        audioElement.load(); // Important to release resources
        audioElement.remove();
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
        startAudio();
    }
}

// Event listeners
startBtn.addEventListener('click', toggleConnection);

volumeControl.addEventListener('input', () => {
    const audioElement = document.getElementById('audio-stream');
    if (audioElement) {
        audioElement.volume = volumeControl.value;
        logDebug(`Volume set to ${volumeControl.value}`, 'audio');
    }
    
    localStorage.setItem('radioVolume', volumeControl.value);
});

muteBtn.addEventListener('click', () => {
    const audioElement = document.getElementById('audio-stream');
    if (audioElement) {
        audioElement.muted = !audioElement.muted;
        muteBtn.textContent = audioElement.muted ? 'Unmute' : 'Mute';
        logDebug(`Audio ${audioElement.muted ? 'muted' : 'unmuted'}`, 'audio');
    }
});

// Handle page visibility
document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') {
        logDebug('Page is now visible', 'general');
        
        // Update now playing
        updateNowPlaying();
        
        // Reconnect if needed and if the user was previously connected
        if (startBtn.dataset.connected === 'true' && (!ws || ws.readyState !== WebSocket.OPEN)) {
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
    
    if (ws) {
        ws.close();
    }
    
    if (audioContext) {
        audioContext.close();
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