import { createContext, useContext, type ReactNode } from 'react';
import type { Api } from './api';

const ApiContext = createContext<Api | null>(null);

/** Supplies the backend to the component tree; tests supply a fake. */
export function ApiProvider({ api, children }: { api: Api; children: ReactNode }) {
  return <ApiContext.Provider value={api}>{children}</ApiContext.Provider>;
}

export function useApi(): Api {
  const api = useContext(ApiContext);
  if (!api) {
    throw new Error('useApi must be used inside an ApiProvider');
  }
  return api;
}
