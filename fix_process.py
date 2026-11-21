with open(r'C:\Users\nanda\Desktop\Github\SKYIOUS KERNEL\kernel\src\task\process.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# Find the first occurrence of the argc push
idx1 = content.index('push_u64(argv.len() as u64, &mut rsp)?;')
# Find the second occurrence
idx2 = content.index('push_u64(argv.len() as u64, &mut rsp)?;', idx1 + 1)

# Find the Ok(rsp) after the second occurrence
idx3 = content.index('Ok(rsp)', idx2)

# Keep everything before idx1, and everything from idx3 onwards
new_content = content[:idx1] + content[idx3:]

with open(r'C:\Users\nanda\Desktop\Github\SKYIOUS KERNEL\kernel\src\task\process.rs', 'w', encoding='utf-8') as f:
    f.write(new_content)

print("Done")