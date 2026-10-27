export function readinessURL(record) {
  if(record.length>128)throw Error("Readiness record exceeds its limit");
  if(!record.endsWith("\n"))return null;
  const match=/^ruvane-ready\t1\t127\.0\.0\.1:([1-9][0-9]{0,4})\n$/.exec(record);
  if(!match||match[0]!==record||Number(match[1])>65535)throw Error("Invalid readiness record");
  return `http://127.0.0.1:${match[1]}`;
}
