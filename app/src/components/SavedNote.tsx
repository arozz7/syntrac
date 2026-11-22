import type { InvestigateSubject } from '../api/aiTypes';
import { formatDateTime } from '../format';
import { subjectKey, useAiEnabled } from './AiEnabled';

/**
 * A small mark beside a program or finding that was investigated before, so its kept answer is easy to spot. Given several subjects (the
 * programs of a group) it marks the group when any of them has one. Nothing while there is none.
 */
export function SavedNote({ subject }: { subject: InvestigateSubject | InvestigateSubject[] }) {
  const { saved } = useAiEnabled();
  const subjects = Array.isArray(subject) ? subject : [subject];
  const times = subjects.map((one) => saved.get(subjectKey(one))).filter((when): when is number => when !== undefined);
  if (times.length === 0) {
    return null;
  }
  const newest = Math.max(...times);
  const which = subjects.length > 1 ? `For ${times.length} of these programs, an answer` : 'An answer';
  return (
    <span
      className="ai-note"
      title={`${which} from your model was saved on ${formatDateTime(newest)}. Open it and press Investigate to read it.`}
    >
      AI note
    </span>
  );
}
