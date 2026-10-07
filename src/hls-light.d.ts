// hls.js ships no typings for its light build; it is the same API as the full one.
declare module 'hls.js/light' {
  import Hls from 'hls.js';
  export default Hls;
}
