/// <reference types="vite/client" />

interface Window {
  __TAURI__: {
    core: {
      invoke<T>(
        command: string,
        parameters?: Record<string, unknown>,
      ): Promise<T>;
    };
  };
}
