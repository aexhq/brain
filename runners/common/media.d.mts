export function loadImages(media?: { type: string; url: string }[], origins?: string[], signal?: AbortSignal): Promise<{ type: "image"; mimeType: string; data: string }[]>;
