/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_CLOUD_API_BASE?: string;
  readonly VITE_HOSTED_APP_HOSTNAMES?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
