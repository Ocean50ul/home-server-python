// DOM state
const playButton = document.getElementById("playButton");
const stopButton = document.getElementById("stopButton");
playButton.disabled = false;
stopButton.disabled = true;

// Web socket connection
const wsProtocol = window.location.protocol === "https:" ? "wss:" : "ws:";
const wsUrl = `${wsProtocol}//${window.location.host}/api/ws/audio`;
const websocket = new WebSocket(wsUrl);
websocket.binaryType = "arraybuffer";

// playback state
let isLoopActive = false;
let audioContext = null;
const jitterThreshold = 10; // jitter buffer size
const maxBufferSize = 150; // max size to avouid memory leaks
const audioQueue = []; // FIFO queue via push and shift
let nextPlayTime = 0;

// Decoder
const decoder = new window["opus-decoder"].OpusDecoder({
  sampleRate: 48000,
  channels: 2,
});

await decoder.ready;
console.log("Decoder is ready!");

// websocket events setup
websocket.onopen = function () {
  console.log("Connected to the websocket!");
};

websocket.onmessage = function (event) {
  // back-pressure
  if (audioQueue.length >= maxBufferSize) {
    console.warn("Buffer full! Dropping incoming audio frame.");
    return;
  }

  // creates a lightweight view
  const opusFrame = new Uint8Array(event.data);
  audioQueue.push(opusFrame);

  // starts the loop if its not active and queue is pre-buffered enough
  // if (audioQueue.length > 0) {... setTimeout(playbackLoop, 10);} keeps loop alive until there is someting
  // inside the audioQueue buffer
  if (!isLoopActive && audioQueue.length >= jitterThreshold) {
    isLoopActive = true;
    playbackLoop();
  }
};

websocket.onclose = function (event) {
  console.error("WebSocket connection closed.", event);
  isLoopActive = false;
  audioQueue.length = 0;
  playButton.disabled = true;
  stopButton.disabled = true;
};

websocket.onerror = function (error) {
  console.error("A WebSocket error occurred:", error);
};

// button clicks handlers
playButton.addEventListener("click", async () => {
  websocket.send(JSON.stringify({ command: "start" }));

  if (!audioContext) {
    audioContext = new AudioContext();
  }

  if (audioContext.state === "suspended") {
    await audioContext.resume();
  }

  playButton.disabled = true;
  stopButton.disabled = false;
});

stopButton.addEventListener("click", () => {
  websocket.send(JSON.stringify({ command: "stop" }));

  playButton.disabled = false;
  stopButton.disabled = true;
});

async function playbackLoop() {
  // we have some work
  if (audioQueue.length > 0) {
    const frameToPlay = audioQueue.shift(); // first out, popping the first element

    const { channelData, samplesDecoded, sampleRate } =
      await decoder.decodeFrame(frameToPlay);

    if (samplesDecoded > 0) {
      const audioBuffer = audioContext.createBuffer(
        channelData.length,
        samplesDecoded,
        sampleRate,
      );

      // channelData is array of arrays: for each channel there is separate array with PCMs.
      for (let i = 0; i < channelData.length; i++) {
        audioBuffer.copyToChannel(channelData[i], i);
      }

      // Source node is something like a mini player
      const sourceNode = audioContext.createBufferSource();
      sourceNode.buffer = audioBuffer;
      sourceNode.connect(audioContext.destination);

      // we need nextPlayTime to make playback smooth
      // basically we need to keep account for the internal to the stream time
      // and play the next chunk exactly after the last one
      if (nextPlayTime < audioContext.currentTime) {
        nextPlayTime = audioContext.currentTime;
      }

      sourceNode.start(nextPlayTime);
      nextPlayTime += audioBuffer.duration;
    }

    // timeout to avoid busy-wait loop
    // basically this thing is scheduling the function to be evoked again every 10ms
    setTimeout(playbackLoop, 10);
  } else {
    // we have no work, the buffer is empty
    isLoopActive = false;
    return;
  }
}
