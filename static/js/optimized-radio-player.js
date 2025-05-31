// static/js/optimized-radio-player.js - Fixed UI disconnect bug

// Radio state management
const radioState = {
    audioElement: null,
    isPlaying: false,
    isMuted: false,
    volume: 0.7,
    currentTrack: null,
    serverPosition: 0,
    isIOS: /iPad|iPhone|iPod/.test(navigator.userAgent),
    isMobile: /Mobi|Android/i.test(navigator.userAgent),
    
    // Timers
    nowPlayingTimer: null,
    heartbeatTimer: null,
    
    // Connection
    connectionId: null,
    consecutiveErrors: 0,
    isReconnecting: false,
    lastSuccessfulConnection: null,
    
    // UI state
    isTogglingConnection: false  // NEW: Prevent rapid toggle clicks
};

// UI Elements
const elements = {
    startBtn: null,
    muteBtn: null,
    volumeControl: null,
    statusMessage: null,
    listenerCount: null,
    currentTitle: null,
    currentArtist: null,
    currentAlbum: null,
    currentPosition: null,
    currentDuration: null,
    progressBar: null
};

// Initialize the radio player
function initRadioPlayer() {
    console.log('🎵 ChillOut Radio - Initializing v2.6.0...');
    
    // Get UI elements
    elements.startBtn = document.getElementById('start-btn');
    elements.muteBtn = document.getElementById('mute-btn');
    elements.volumeControl = document.getElementById('volume');
    elements.statusMessage = document.getElementById('status-message');
    elements.listenerCount = document.getElementById('listener-count');
    elements.currentTitle = document.getElementById('current-title');
    elements.currentArtist = document.getElementById('current-artist');
    elements.currentAlbum = document.getElementById('current-album');
    elements.currentPosition = document.getElementById('current-position');
    elements.currentDuration = document.getElementById('current-duration');
    elements.progressBar = document.getElementById('progress-bar');
    
    // Check required elements
    if (!elements.startBtn || !elements.muteBtn || !elements.volumeControl) {
        console.error('❌ Required UI elements not found');
        showStatus('❌ Player initialization failed - missing UI elements', true);
        return;
    }
    
    // Set up event listeners
    setupEventListeners();
    
    // Load saved settings
    loadSettings();
    
    // Start fetching track info
    startTrackInfoUpdates();
    
    // Initial health check
    performHealthCheck();
    
    console.log('✅ Radio player initialized successfully');
    showStatus('📻 Radio ready - click "Tune In" to start listening');
}

// Set up event listeners with proper UI state handling
function setupEventListeners() {
    // Main toggle button with debouncing
    elements.startBtn.addEventListener('click', (event) => {
        event.preventDefault();
        event.stopPropagation();
        
        // Prevent rapid clicking
        if (radioState.isTogglingConnection) {
            console.log('⏸️ Ignoring click - already toggling connection');
            return;
        }
        
        toggleRadio();
    });
    
    elements.muteBtn.addEventListener('click', toggleMute);
    elements.volumeControl.addEventListener('input', (e) => {
        updateVolume(parseFloat(e.target.value));
    });
    
    // Handle page visibility changes
    document.addEventListener('visibilitychange', () => {
        if (document.hidden && radioState.isPlaying) {
            console.log('📱 Page hidden - maintaining radio connection');
        } else if (!document.hidden && radioState.isPlaying) {
            console.log('📱 Page visible - checking radio connection');
            checkConnectionHealth();
        }
    });
}

// Perform initial health check
async function performHealthCheck() {
    try {
        console.log('🔍 Performing server health check...');
        const response = await fetch('/api/health', {
            headers: { 'Cache-Control': 'no-cache' }
        });
        
        if (response.ok) {
            const data = await response.json();
            console.log('✅ Server health check passed:', data);
            showStatus(`📻 Server ready - ${data.active_listeners || 0} listeners online`);
        } else {
            console.warn('⚠️ Server health check failed:', response.status);
            showStatus('⚠️ Server connection issues detected', true);
        }
    } catch (error) {
        console.error('❌ Health check failed:', error);
        showStatus('❌ Cannot connect to radio server', true);
    }
}

// Toggle radio on/off with proper state management
async function toggleRadio() {
    console.log('🔄 Toggle radio clicked, current state:', {
        isPlaying: radioState.isPlaying,
        isReconnecting: radioState.isReconnecting,
        isTogglingConnection: radioState.isTogglingConnection
    });
    
    // Set toggle lock
    radioState.isTogglingConnection = true;
    
    try {
        if (radioState.isPlaying) {
            await stopRadio();
        } else {
            await startRadio();
        }
    } catch (error) {
        console.error('❌ Error during toggle:', error);
        showStatus(`❌ Toggle failed: ${error.message}`, true);
    } finally {
        // Release toggle lock after a delay
        setTimeout(() => {
            radioState.isTogglingConnection = false;
        }, 500);
    }
}

// Start radio with improved error handling
async function startRadio() {
    console.log('🎵 Starting radio...');
    
    if (radioState.isPlaying || radioState.isReconnecting) {
        console.log('Already playing or reconnecting');
        return;
    }
    
    // Update state immediately
    radioState.isPlaying = true;
    radioState.isReconnecting = false;
    radioState.consecutiveErrors = 0;
    
    // Update UI immediately
    updateUIForConnecting();
    
    try {
        // Create audio element
        await createAudioElement();
        
        // Start streaming
        await startStreaming();
        
        // Update UI on success
        updateUIForConnected();
        
    } catch (error) {
        console.error('❌ Failed to start radio:', error);
        radioState.isPlaying = false;
        updateUIForDisconnected();
        showStatus(`❌ Failed to start: ${error.message}`, true);
        throw error; // Re-throw for toggle handler
    }
}

// Update UI functions for better state management
function updateUIForConnecting() {
    elements.startBtn.disabled = true;
    elements.startBtn.textContent = '📻 Connecting...';
    elements.startBtn.dataset.connected = 'connecting';
    showStatus('📻 Connecting to radio stream...');
}

function updateUIForConnected() {
    elements.startBtn.disabled = false;
    elements.startBtn.textContent = '📻 Disconnect';
    elements.startBtn.dataset.connected = 'true';
    showStatus('📻 🎵 Live on ChillOut Radio!');
}

function updateUIForDisconnected() {
    elements.startBtn.disabled = false;
    elements.startBtn.textContent = '📻 Tune In';
    elements.startBtn.dataset.connected = 'false';
}

// Create audio element with better error handling
async function createAudioElement() {
    console.log('🔊 Creating audio element...');
    
    // Clean up existing element more thoroughly
    if (radioState.audioElement) {
        try {
            radioState.audioElement.pause();
            radioState.audioElement.src = '';
            radioState.audioElement.load(); // Force cleanup
            radioState.audioElement.removeEventListener('error', handleAudioError);
            
            // Remove all event listeners
            const events = ['loadstart', 'loadedmetadata', 'canplay', 'canplaythrough', 
                          'play', 'playing', 'pause', 'waiting', 'stalled', 'suspend', 
                          'progress', 'timeupdate', 'ended', 'emptied', 'abort'];
            
            events.forEach(event => {
                radioState.audioElement.removeEventListener(event, () => {});
            });
            
        } catch (error) {
            console.warn('⚠️ Error during audio cleanup:', error);
        }
        
        radioState.audioElement = null;
    }
    
    // Small delay to ensure cleanup is complete
    await new Promise(resolve => setTimeout(resolve, 100));
    
    // Create new audio element
    try {
        radioState.audioElement = new Audio();
        
        // Verify the element was created successfully
        if (!radioState.audioElement) {
            throw new Error('Failed to create Audio element');
        }
        
        // Set basic properties
        radioState.audioElement.volume = radioState.volume;
        radioState.audioElement.muted = radioState.isMuted;
        radioState.audioElement.crossOrigin = "anonymous";
        
        // Platform-specific settings
        if (radioState.isIOS) {
            radioState.audioElement.preload = 'none';
            radioState.audioElement.playsInline = true;
        } else {
            radioState.audioElement.preload = 'metadata';
        }
        
        // Set up comprehensive event listeners
        setupAudioEventListeners();
        
        console.log('✅ Audio element created successfully');
        
    } catch (error) {
        console.error('❌ Failed to create audio element:', error);
        radioState.audioElement = null;
        throw new Error(`Audio creation failed: ${error.message}`);
    }
    
    return Promise.resolve();
}

// Set up comprehensive audio event listeners
function setupAudioEventListeners() {
    const audio = radioState.audioElement;
    
    // Loading events
    audio.addEventListener('loadstart', () => {
        console.log('🔄 Audio: Load started');
        showStatus('📻 Connecting to stream...');
    });
    
    audio.addEventListener('loadedmetadata', () => {
        console.log('📊 Audio: Metadata loaded');
    });
    
    audio.addEventListener('canplay', () => {
        console.log('✅ Audio: Can play');
        showStatus('📻 Stream ready to play...');
    });
    
    audio.addEventListener('canplaythrough', () => {
        console.log('✅ Audio: Can play through');
    });
    
    // Playback events
    audio.addEventListener('play', () => {
        console.log('▶️ Audio: Play event');
    });
    
    audio.addEventListener('playing', () => {
        console.log('▶️ Audio: Playing');
        showStatus('📻 🎵 Live on ChillOut Radio!');
        radioState.consecutiveErrors = 0;
        radioState.lastSuccessfulConnection = Date.now();
    });
    
    audio.addEventListener('pause', () => {
        console.log('⏸️ Audio: Paused');
    });
    
    // Buffering events
    audio.addEventListener('waiting', () => {
        console.log('⏳ Audio: Waiting/Buffering');
        showStatus('📻 Buffering...');
    });
    
    audio.addEventListener('stalled', () => {
        console.log('⚠️ Audio: Stalled');
        showStatus('📻 Connection slow - buffering...');
    });
    
    audio.addEventListener('suspend', () => {
        console.log('⏸️ Audio: Suspended');
    });
    
    // Progress events
    audio.addEventListener('progress', () => {
        console.log('📊 Audio: Progress');
    });
    
    audio.addEventListener('timeupdate', () => {
        // Don't log this one as it's too frequent
    });
    
    // Error handling
    audio.addEventListener('error', handleAudioError);
    
    // End events
    audio.addEventListener('ended', () => {
        console.log('🔄 Audio: Ended - this should not happen in radio mode');
        if (radioState.isPlaying) {
            scheduleReconnect('Stream ended unexpectedly');
        }
    });
    
    // Network state changes
    audio.addEventListener('emptied', () => {
        console.log('📭 Audio: Emptied');
    });
    
    audio.addEventListener('abort', () => {
        console.log('🛑 Audio: Aborted');
    });
}

// Handle audio errors with detailed logging
function handleAudioError(e) {
    const error = e.target.error;
    let errorMsg = 'Unknown error';
    let shouldReconnect = true;
    
    if (error) {
        switch (error.code) {
            case MediaError.MEDIA_ERR_ABORTED:
                errorMsg = 'Playback aborted by user';
                shouldReconnect = false;
                break;
            case MediaError.MEDIA_ERR_NETWORK:
                errorMsg = 'Network error while loading';
                break;
            case MediaError.MEDIA_ERR_DECODE:
                errorMsg = 'Audio decoding error';
                break;
            case MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED:
                errorMsg = 'Audio format not supported';
                shouldReconnect = false;
                break;
            default:
                errorMsg = `Media error (code ${error.code})`;
        }
    }
    
    console.error('❌ Audio error:', errorMsg, error);
    radioState.consecutiveErrors++;
    
    // Only handle errors if we're still supposed to be playing
    if (!radioState.isPlaying) {
        console.log('⏭️ Ignoring audio error - player was stopped');
        return;
    }
    
    // Show appropriate error message
    if (error && error.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED) {
        showStatus('❌ Audio format not supported by your browser', true);
        stopRadio(true);
    } else if (error && error.code === MediaError.MEDIA_ERR_NETWORK) {
        showStatus('📻 Network error - attempting to reconnect...', true);
        if (shouldReconnect && radioState.consecutiveErrors < 5) {
            scheduleReconnect('Network error');
        } else {
            stopRadio(true);
        }
    } else {
        showStatus(`❌ Playback error: ${errorMsg}`, true);
        if (shouldReconnect && radioState.consecutiveErrors < 3) {
            scheduleReconnect(errorMsg);
        } else {
            stopRadio(true);
        }
    }
}

// Start streaming with better error handling
async function startStreaming() {
    // Verify audio element exists
    if (!radioState.audioElement) {
        throw new Error('Audio element is null - createAudioElement may have failed');
    }
    
    console.log('🌐 Starting streaming with audio element:', radioState.audioElement);
    
    // Check server status first
    try {
        const statusResponse = await fetch('/stream-status', {
            headers: { 'Cache-Control': 'no-cache' }
        });
        
        if (!statusResponse.ok) {
            throw new Error(`Server returned ${statusResponse.status}`);
        }
        
        const statusData = await statusResponse.json();
        if (!statusData.streaming) {
            throw new Error('Server is not currently streaming');
        }
        
        console.log('✅ Server streaming status confirmed:', statusData);
    } catch (error) {
        console.error('❌ Server status check failed:', error);
        throw new Error(`Server not ready: ${error.message}`);
    }
    
    // Create stream URL with cache busting and platform info
    const timestamp = Date.now();
    let streamUrl = `/direct-stream?t=${timestamp}`;
    
    // Add platform info
    if (radioState.isIOS) {
        streamUrl += '&platform=ios';
    } else if (radioState.isMobile) {
        streamUrl += '&platform=mobile';
    } else {
        streamUrl += '&platform=desktop';
    }
    
    console.log('🌐 Stream URL:', streamUrl);
    
    // Verify audio element still exists before setting src
    if (!radioState.audioElement) {
        throw new Error('Audio element became null during setup');
    }
    
    // Set source with error handling
    try {
        radioState.audioElement.src = streamUrl;
        console.log('✅ Audio src set successfully');
    } catch (error) {
        console.error('❌ Failed to set audio src:', error);
        throw new Error(`Failed to set audio source: ${error.message}`);
    }
    
    // Start playback with timeout
    return new Promise((resolve, reject) => {
        let timeoutId;
        let resolved = false;
        
        const cleanup = () => {
            if (timeoutId) clearTimeout(timeoutId);
        };
        
        const handleSuccess = () => {
            if (resolved) return;
            resolved = true;
            cleanup();
            console.log('✅ Streaming started successfully');
            resolve();
        };
        
        const handleError = (error) => {
            if (resolved) return;
            resolved = true;
            cleanup();
            console.error('❌ Streaming failed:', error);
            reject(error);
        };
        
        // Set up timeout
        timeoutId = setTimeout(() => {
            handleError(new Error('Connection timeout after 10 seconds'));
        }, 10000);
        
        // Verify audio element exists before adding listeners
        if (!radioState.audioElement) {
            handleError(new Error('Audio element is null'));
            return;
        }
        
        // Set up success listener
        const playingHandler = () => {
            if (radioState.audioElement) {
                radioState.audioElement.removeEventListener('playing', playingHandler);
            }
            handleSuccess();
        };
        
        radioState.audioElement.addEventListener('playing', playingHandler);
        
        // Attempt to play
        const playPromise = radioState.audioElement.play();
        
        if (playPromise !== undefined) {
            playPromise.then(() => {
                console.log('✅ Play promise resolved');
                // Success will be handled by 'playing' event
            }).catch(error => {
                console.error('❌ Play promise rejected:', error);
                
                if (error.name === 'NotAllowedError') {
                    // Autoplay blocked - need user interaction
                    resolved = true;
                    cleanup();
                    if (radioState.audioElement) {
                        radioState.audioElement.removeEventListener('playing', playingHandler);
                    }
                    
                    showStatus('🔊 Click to enable audio playback', true);
                    elements.startBtn.textContent = '🔊 Enable Audio';
                    elements.startBtn.disabled = false;
                    
                    elements.startBtn.onclick = async () => {
                        try {
                            if (radioState.audioElement) {
                                await radioState.audioElement.play();
                                elements.startBtn.onclick = null; // Clear this handler
                                setupEventListeners(); // Restore normal handlers
                                updateUIForConnected();
                                showStatus('📻 🎵 Live on ChillOut Radio!');
                                resolve();
                            } else {
                                reject(new Error('Audio element is null during manual play'));
                            }
                        } catch (playError) {
                            console.error('❌ Manual play failed:', playError);
                            reject(playError);
                        }
                    };
                } else {
                    handleError(error);
                }
            });
        }
    });
}

// Stop radio with comprehensive cleanup
async function stopRadio(isError = false) {
    console.log('⏹️ Stopping radio...');
    
    // Update state immediately
    radioState.isPlaying = false;
    radioState.isReconnecting = false;
    
    // Comprehensive audio cleanup
    if (radioState.audioElement) {
        try {
            // Pause and clear source
            radioState.audioElement.pause();
            radioState.audioElement.src = '';
            radioState.audioElement.load(); // Force resource cleanup
            
            // Remove specific event listeners we know about
            radioState.audioElement.removeEventListener('error', handleAudioError);
            
            // Remove all possible event listeners
            const events = ['loadstart', 'loadedmetadata', 'canplay', 'canplaythrough', 
                          'play', 'playing', 'pause', 'waiting', 'stalled', 'suspend', 
                          'progress', 'timeupdate', 'ended', 'emptied', 'abort'];
            
            events.forEach(event => {
                // Clone and replace the element to remove all listeners
                const newAudio = radioState.audioElement.cloneNode(false);
                if (radioState.audioElement.parentNode) {
                    radioState.audioElement.parentNode.replaceChild(newAudio, radioState.audioElement);
                }
            });
            
        } catch (error) {
            console.warn('⚠️ Error during audio cleanup:', error);
        }
        
        radioState.audioElement = null;
    }
    
    // Clear timers
    clearTimers();
    
    // Reset connection state
    radioState.connectionId = null;
    radioState.consecutiveErrors = 0;
    
    // Update UI
    updateUIForDisconnected();
    
    if (!isError) {
        showStatus('📻 Disconnected from radio');
        console.log('✅ Radio stopped cleanly');
    } else {
        console.log('⚠️ Radio stopped due to error');
    }
}

// Schedule reconnect with exponential backoff and better state management
function scheduleReconnect(reason = 'Unknown error') {
    if (radioState.isReconnecting || !radioState.isPlaying) {
        console.log('⏭️ Skipping reconnect - already reconnecting or not playing');
        return;
    }
    
    radioState.isReconnecting = true;
    console.log(`🔄 Scheduling reconnect due to: ${reason}`);
    
    // Clean up current audio element first
    if (radioState.audioElement) {
        try {
            radioState.audioElement.pause();
            radioState.audioElement.src = '';
            radioState.audioElement.load();
            radioState.audioElement = null;
        } catch (error) {
            console.warn('⚠️ Error during reconnect cleanup:', error);
        }
    }
    
    // Exponential backoff: 1s, 2s, 4s, 8s, max 10s
    const delay = Math.min(1000 * Math.pow(2, radioState.consecutiveErrors), 10000);
    
    showStatus(`📻 Reconnecting in ${Math.round(delay/1000)}s... (${reason})`);
    
    setTimeout(async () => {
        if (radioState.isPlaying && radioState.isReconnecting) {
            console.log(`🔄 Attempting reconnect (attempt ${radioState.consecutiveErrors + 1})`);
            
            try {
                radioState.isReconnecting = false;
                
                // Create fresh audio element
                await createAudioElement();
                
                // Verify element was created
                if (!radioState.audioElement) {
                    throw new Error('Failed to create audio element during reconnect');
                }
                
                // Start streaming
                await startStreaming();
                showStatus('📻 🎵 Reconnected successfully!');
                radioState.consecutiveErrors = 0; // Reset on success
                
            } catch (error) {
                console.error('❌ Reconnect failed:', error);
                radioState.consecutiveErrors++;
                
                if (radioState.consecutiveErrors >= 5) {
                    console.error('❌ Max reconnection attempts reached');
                    showStatus('❌ Connection failed - please try again later', true);
                    await stopRadio(true);
                } else {
                    scheduleReconnect('Reconnection failed');
                }
            }
        } else {
            console.log('⏭️ Reconnect cancelled - player stopped or already reconnecting');
            radioState.isReconnecting = false;
        }
    }, delay);
}

// Check connection health
async function checkConnectionHealth() {
    if (!radioState.isPlaying) return;
    
    try {
        const response = await fetch('/api/heartbeat', {
            headers: { 'Cache-Control': 'no-cache' }
        });
        
        if (!response.ok) {
            console.warn('⚠️ Heartbeat failed:', response.status);
            return;
        }
        
        const data = await response.json();
        
        // Update listener count from heartbeat
        if (data.active_listeners !== undefined && elements.listenerCount) {
            elements.listenerCount.innerHTML = `<span class="radio-live">LIVE</span> • Listeners: ${data.active_listeners}`;
        }
        
        console.log('💓 Connection healthy, listeners:', data.active_listeners);
        
    } catch (error) {
        console.error('❌ Health check failed:', error);
    }
}

// Toggle mute
function toggleMute() {
    radioState.isMuted = !radioState.isMuted;
    
    if (radioState.audioElement) {
        radioState.audioElement.muted = radioState.isMuted;
    }
    
    elements.muteBtn.textContent = radioState.isMuted ? '🔇 Unmute' : '🔊 Mute';
    
    // Save setting
    try {
        localStorage.setItem('radioMuted', radioState.isMuted.toString());
    } catch (e) {
        console.log('Could not save mute setting');
    }
}

// Update volume
function updateVolume(volume) {
    radioState.volume = volume;
    
    if (radioState.audioElement) {
        radioState.audioElement.volume = volume;
    }
    
    // Save setting
    try {
        localStorage.setItem('radioVolume', volume.toString());
    } catch (e) {
        console.log('Could not save volume setting');
    }
}

// Load settings
function loadSettings() {
    try {
        const savedVolume = localStorage.getItem('radioVolume');
        if (savedVolume !== null) {
            const volume = parseFloat(savedVolume);
            if (!isNaN(volume) && volume >= 0 && volume <= 1) {
                elements.volumeControl.value = volume;
                radioState.volume = volume;
            }
        }
        
        const savedMuted = localStorage.getItem('radioMuted');
        if (savedMuted !== null) {
            radioState.isMuted = savedMuted === 'true';
            elements.muteBtn.textContent = radioState.isMuted ? '🔇 Unmute' : '🔊 Mute';
        }
    } catch (e) {
        console.log('Could not load settings from localStorage');
    }
}

// Start track info updates
function startTrackInfoUpdates() {
    // Clear existing timers
    clearTimers();
    
    // Fetch now playing every 5 seconds (reduced from 8)
    radioState.nowPlayingTimer = setInterval(fetchNowPlaying, 5000);
    
    // Send heartbeat every 15 seconds
    radioState.heartbeatTimer = setInterval(sendHeartbeat, 15000);
    
    // Initial fetch
    fetchNowPlaying();
}

// Clear timers
function clearTimers() {
    if (radioState.nowPlayingTimer) {
        clearInterval(radioState.nowPlayingTimer);
        radioState.nowPlayingTimer = null;
    }
    if (radioState.heartbeatTimer) {
        clearInterval(radioState.heartbeatTimer);
        radioState.heartbeatTimer = null;
    }
}

// Fetch now playing with better error handling
async function fetchNowPlaying() {
    try {
        const response = await fetch('/api/now-playing', {
            headers: {
                'Cache-Control': 'no-cache',
                'Accept': 'application/json'
            }
        });
        
        if (!response.ok) {
            console.error('❌ Now playing API error:', response.status);
            return;
        }
        
        const data = await response.json();
        updateTrackInfo(data);
        
    } catch (error) {
        console.error('❌ Error fetching now playing:', error);
        // Don't show error to user for this - it's not critical
    }
}

// Update track info with comprehensive error handling
function updateTrackInfo(info) {
    try {
        if (!info || typeof info !== 'object') {
            console.warn('⚠️ Invalid track info received:', info);
            return;
        }
        
        if (info.error) {
            console.error('❌ Server error in track info:', info.error);
            return;
        }
        
        radioState.currentTrack = info;
        radioState.serverPosition = info.radio_position || info.playback_position || 0;
        
        // Update track display
        if (elements.currentTitle && info.title) {
            elements.currentTitle.textContent = info.title;
        }
        if (elements.currentArtist && info.artist) {
            elements.currentArtist.textContent = info.artist;
        }
        if (elements.currentAlbum && info.album) {
            elements.currentAlbum.textContent = info.album;
        }
        
        // Update duration
        if (elements.currentDuration && info.duration && info.duration > 0) {
            elements.currentDuration.textContent = formatTime(info.duration);
        }
        
        // Update position
        if (elements.currentPosition) {
            elements.currentPosition.textContent = formatTime(radioState.serverPosition);
        }
        
        // Update progress bar
        if (elements.progressBar && info.duration && info.duration > 0) {
            const percent = (radioState.serverPosition / info.duration) * 100;
            elements.progressBar.style.width = `${Math.min(100, Math.max(0, percent))}%`;
        }
        
        // Update listener count
        if (elements.listenerCount && typeof info.active_listeners === 'number') {
            elements.listenerCount.innerHTML = `<span class="radio-live">LIVE</span> • Listeners: ${info.active_listeners}`;
        }
        
        // Update document title
        if (info.title && info.artist) {
            document.title = `📻 ${info.title} - ${info.artist} | ChillOut Radio`;
        }
        
        // Log successful update (occasionally)
        if (Math.random() < 0.1) { // 10% of the time
            console.log('📊 Track info updated:', {
                title: info.title,
                position: radioState.serverPosition,
                listeners: info.active_listeners
            });
        }
        
    } catch (error) {
        console.error('❌ Error updating track info:', error);
    }
}

// Send heartbeat
async function sendHeartbeat() {
    if (!radioState.connectionId) {
        return;
    }
    
    try {
        const response = await fetch(`/api/heartbeat?connection_id=${radioState.connectionId}`, {
            headers: { 'Cache-Control': 'no-cache' }
        });
        
        if (response.ok) {
            const data = await response.json();
            
            // Update listener count from heartbeat
            if (data.active_listeners !== undefined && elements.listenerCount) {
                elements.listenerCount.innerHTML = `<span class="radio-live">LIVE</span> • Listeners: ${data.active_listeners}`;
            }
        }
    } catch (error) {
        console.log('💓 Heartbeat failed (this is OK):', error.message);
    }
}

// Show status message
function showStatus(message, isError = false, autoHide = true) {
    console.log(`Status: ${message}`);
    
    if (elements.statusMessage) {
        elements.statusMessage.textContent = message;
        elements.statusMessage.style.display = 'block';
        elements.statusMessage.style.borderLeftColor = isError ? '#e74c3c' : '#4a90e2';
        elements.statusMessage.style.backgroundColor = isError ? '#fdf2f2' : '#f0f8ff';
        
        if (!isError && autoHide) {
            setTimeout(() => {
                if (elements.statusMessage.textContent === message) {
                    elements.statusMessage.style.display = 'none';
                }
            }, 4000);
        }
    }
}

// Format time helper
function formatTime(seconds) {
    if (!seconds || seconds < 0 || !isFinite(seconds)) return '0:00';
    const minutes = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

// Initialize when page loads
document.addEventListener('DOMContentLoaded', () => {
    try {
        initRadioPlayer();
        console.log('🎵 ChillOut Radio v2.6.0 - UI Bug Fixed');
    } catch (error) {
        console.error('❌ Failed to initialize radio player:', error);
        alert('Radio player failed to initialize. Please refresh the page.');
    }
});

// Cleanup on page unload
window.addEventListener('beforeunload', () => {
    console.log('📱 Page unloading - cleaning up');
    clearTimers();
    if (radioState.audioElement) {
        radioState.audioElement.pause();
        radioState.audioElement.src = '';
    }
});

// Handle online/offline events
window.addEventListener('online', () => {
    console.log('🌐 Network: Online');
    if (radioState.isPlaying && !radioState.audioElement) {
        showStatus('📻 Network restored - reconnecting...', false);
        scheduleReconnect('Network restored');
    }
});

window.addEventListener('offline', () => {
    console.log('📡 Network: Offline');
    showStatus('📡 Network offline - will reconnect when available', true);
});

// Debug object for troubleshooting
window.ChillOutRadio = {
    state: radioState,
    elements: elements,
    start: startRadio,
    stop: stopRadio,
    fetchInfo: fetchNowPlaying,
    healthCheck: performHealthCheck,
    version: '2.6.0-ui-fix',
    
    // Debug methods
    getAudioState: () => {
        if (!radioState.audioElement) return 'No audio element (NULL)';
        return {
            paused: radioState.audioElement.paused,
            ended: radioState.audioElement.ended,
            error: radioState.audioElement.error,
            readyState: radioState.audioElement.readyState,
            networkState: radioState.audioElement.networkState,
            src: radioState.audioElement.src,
            srcIsNull: radioState.audioElement.src === null,
            srcIsEmpty: radioState.audioElement.src === ''
        };
    },
    
    reconnect: () => scheduleReconnect('Manual reconnect'),
    
    testStream: async () => {
        try {
            const response = await fetch('/stream-status');
            const data = await response.json();
            console.log('Stream test result:', data);
            return data;
        } catch (error) {
            console.error('Stream test failed:', error);
            return { error: error.message };
        }
    },
    
    // New debug methods for null audio element issues
    forceCleanup: () => {
        console.log('🧹 Force cleanup requested');
        stopRadio(false);
    },
    
    checkAudioElementStatus: () => {
        console.log('🔍 Audio element status:');
        console.log('  - radioState.audioElement:', radioState.audioElement);
        console.log('  - typeof audioElement:', typeof radioState.audioElement);
        console.log('  - audioElement === null:', radioState.audioElement === null);
        console.log('  - audioElement === undefined:', radioState.audioElement === undefined);
        return {
            exists: !!radioState.audioElement,
            type: typeof radioState.audioElement,
            isNull: radioState.audioElement === null,
            isUndefined: radioState.audioElement === undefined
        };
    },
    
    // UI state debugging
    getUIState: () => {
        return {
            isPlaying: radioState.isPlaying,
            isReconnecting: radioState.isReconnecting,
            isTogglingConnection: radioState.isTogglingConnection,
            buttonText: elements.startBtn ? elements.startBtn.textContent : 'No button',
            buttonDisabled: elements.startBtn ? elements.startBtn.disabled : 'No button',
            buttonDataset: elements.startBtn ? elements.startBtn.dataset.connected : 'No button'
        };
    },
    
    // Force UI reset
    resetUI: () => {
        console.log('🔄 Forcing UI reset');
        radioState.isTogglingConnection = false;
        radioState.isReconnecting = false;
        updateUIForDisconnected();
    }
};

console.log('🎵 ChillOut Radio v2.6.0 - UI Disconnect Bug Fixed');
console.log('🔧 Debug: window.ChillOutRadio available for troubleshooting');
console.log('✅ Fixed: Disconnect button immediately reconnecting');
console.log('🛡️ Added: Toggle connection lock to prevent rapid clicks');