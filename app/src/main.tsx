import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { tauriApi } from './api/api';
import { ApiProvider } from './api/ApiContext';
import './theme/tokens.css';
import './theme/global.css';
import './theme/app.css';
import './theme/ai.css';

const container = document.getElementById('root');
if (!container) {
  throw new Error('Root element #root not found');
}

createRoot(container).render(
  <StrictMode>
    <ApiProvider api={tauriApi}>
      <App />
    </ApiProvider>
  </StrictMode>,
);
