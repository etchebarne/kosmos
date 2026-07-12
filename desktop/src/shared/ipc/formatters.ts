export type FormatterInstallationState =
  | "notInstalled"
  | "installing"
  | "installed"
  | "uninstalling"
  | "failed";

export type FormatterFailure = { code: string; message: string };

export type FormatterSnapshot = {
  id: string;
  name: string;
  description: string;
  languages: string[];
  languageIds: string[];
  extensions: string[];
  filenames: string[];
  priority: number;
  catalogVersion: string;
  installedVersion: string | null;
  installationState: FormatterInstallationState;
  lastError: FormatterFailure | null;
  supported: boolean;
};

export type FormatterListSnapshot = { formatters: FormatterSnapshot[] };
export type FormatterParams = { formatterId: string };
export type FormatterPrioritiesParams = { formatterIds: string[] };
