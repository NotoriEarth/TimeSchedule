import type { EventInput, NoteInput, TaskInput } from './api';
import type { EntryInput, ProjectInput } from './diary-api';
export type EditorRequest = {date:string} & (
  {type:'event';value:EventInput} | {type:'entry';value:EntryInput} | {type:'project';value:ProjectInput} | {type:'note';value:NoteInput} | {type:'task';value:TaskInput}
);
