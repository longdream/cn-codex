let intervalId = null;
export function start(fn, ms) { intervalId = setInterval(fn, ms); }
export function stop() { if (intervalId) { clearInterval(intervalId); intervalId = null; } }
