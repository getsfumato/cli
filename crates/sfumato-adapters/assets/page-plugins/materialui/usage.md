Material UI is loaded offline as `window.MaterialUI`, with React and ReactDOM resolved automatically.

Build the interactive interface with `React.createElement` and components destructured from `MaterialUI`, for example `Container`, `Stack`, `Card`, `Typography`, `Button`, `Slider`, `Tabs`, and `Tooltip`. JSX, imports, npm, and remote assets are unavailable.

Create a Material UI theme with `MaterialUI.createTheme` using the Sfumato theme colors supplied in the prompt, then wrap the app in `MaterialUI.ThemeProvider` and `MaterialUI.CssBaseline`. Mount into `<div id="sfumato-react-root"></div>` with `ReactDOM.createRoot`. Keep meaningful static fallback content inside the root element until React mounts.
