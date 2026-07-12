import type {
  FormatterListSnapshot,
  FormatterParams,
  FormatterPrioritiesParams,
  FormatterSnapshot,
} from "@/shared/ipc";

import { requestServer } from "./transport";

const DOMAIN = "formatters";

export function listFormatters(): Promise<FormatterListSnapshot> {
  return requestServer(DOMAIN, "list");
}

export function getFormatterStatus(params: FormatterParams): Promise<FormatterSnapshot> {
  return requestServer(DOMAIN, "status", params);
}

export function installFormatter(params: FormatterParams): Promise<FormatterSnapshot> {
  return requestServer(DOMAIN, "install", params);
}

export function uninstallFormatter(params: FormatterParams): Promise<FormatterSnapshot> {
  return requestServer(DOMAIN, "uninstall", params);
}

export function setFormatterPriorities(
  params: FormatterPrioritiesParams,
): Promise<FormatterListSnapshot> {
  return requestServer(DOMAIN, "set-priorities", params);
}
