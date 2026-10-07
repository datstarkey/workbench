const userAgent = typeof navigator === 'undefined' ? '' : navigator.userAgent;

export const IS_WINDOWS = userAgent.includes('Windows');
export const IS_MAC = /Macintosh|Mac OS X/.test(userAgent);
