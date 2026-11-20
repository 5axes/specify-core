// The app always loads the engine the command serves (config.wasmUrl), so the dependency's copy is
// left out of the build (see the README, "Serving the engine yourself").
export default function init(): never {
  throw new Error('The Specify app loads the engine from config.wasmUrl.');
}
