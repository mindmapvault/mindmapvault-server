import { useEffect, useState } from 'react';

/** Narrower than Tailwind's `sm` breakpoint — a phone held upright. */
const PHONE_QUERY = '(max-width: 639px)';

/** True while the window is phone-sized, following rotation and resizing. */
export function useIsPhone(): boolean {
  const [isPhone, setIsPhone] = useState(
    () => typeof window !== 'undefined' && window.matchMedia(PHONE_QUERY).matches,
  );
  useEffect(() => {
    const mq = window.matchMedia(PHONE_QUERY);
    const onChange = () => setIsPhone(mq.matches);
    onChange();
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, []);
  return isPhone;
}
