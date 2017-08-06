GA_EXE := guest-agent/VfioService.exe
GA_ISO := guest-agent/windows-gaming-ga.iso
OVMF   := ovmf-x64/OVMF_CODE-pure-efi.fd ovmf-x64/OVMF_VARS-pure-efi.fd
BASH_COMPLETION := target/release/windows-gaming.bash-completion

all: cargo $(GA_ISO) $(OVMF) $(BASH_COMPLETION)

clippy:
	rustup run nightly cargo clippy

test:
	cargo test --all --release --locked && cargo test --all --locked

cargo: # Simply always run cargo instead of tracking all the rs sources in here
	cargo build --all --release --locked

$(GA_EXE): guest-agent/VfioService/VfioService.sln $(wildcard guest-agent/VfioService/VfioService/*.*) $(wildcard guest-agent/VfioService/VfioService/Properties/*)
	cd guest-agent/VfioService && nuget restore
	xbuild /p:Configuration=Release guest-agent/VfioService/VfioService.sln
	cp --preserve=timestamps guest-agent/VfioService/VfioService/bin/x86/Release/VfioService.exe guest-agent
	cp --preserve=timestamps guest-agent/VfioService/VfioService/bin/x86/Release/Google.Protobuf.dll guest-agent

$(GA_ISO): $(GA_EXE) guest-agent/install.bat guest-agent/uninstall.bat
	cd guest-agent && mkisofs -m VfioService -m .gitignore -m update-proto.bat -o windows-gaming-ga.iso -r -J -input-charset iso8859-1 -V "windows-gaming-ga" .

ovmf.rpm:
	curl -o ovmf.rpm "https://www.kraxel.org/repos/jenkins/edk2/$(shell curl -s 'https://www.kraxel.org/repos/jenkins/edk2/' | grep -Eo 'edk2.git-ovmf-x64-[-\.a-z0-9]+\.noarch\.rpm' | head -n1)"

ovmf-x64/OVMF_CODE-pure-efi%fd ovmf-x64/OVMF_VARS-pure-efi%fd: ovmf%rpm
	rpm2cpio ovmf.rpm | bsdtar -xvmf - --strip-components 4 './usr/share/edk2.git/ovmf-x64/OVMF_CODE-pure-efi.fd' './usr/share/edk2.git/ovmf-x64/OVMF_VARS-pure-efi.fd'


guest-agent/VfioService/VfioService/Protocol.cs: windows-gaming/driver/clientpipe-proto/src/protocol.proto
	protoc windows-gaming/driver/clientpipe-proto/src/protocol.proto --csharp_out=guest-agent/VfioService/VfioService/

$(BASH_COMPLETION): cargo
	cd target/release/ && ./windows-gaming --generate-bash-completions > windows-gaming.bash-completion

clean:
	$(RM) ovmf.rpm $(OVMF)
	$(RM) $(GA_EXE) $(GA_ISO)
	cd guest-agent && xbuild /p:Configuration=Release /t:clean VfioService/VfioService.sln
	cargo clean --release



install: all
	install -D target/release/windows-gaming $(DESTDIR)/usr/bin/windows-gaming
	install -D target/release/windows-edge-grab $(DESTDIR)/usr/bin/windows-edge-grab
	install -D -m4755 target/release/vfio-ubind $(DESTDIR)/usr/lib/windows-gaming/vfio-ubind
	install -D -m644 $(BASH_COMPLETION) $(DESTDIR)/usr/share/bash-completion/completions/windows-gaming
	install -D -m644 ovmf-x64/OVMF_CODE-pure-efi.fd $(DESTDIR)/usr/lib/windows-gaming/ovmf-code.fd
	install -D -m644 ovmf-x64/OVMF_VARS-pure-efi.fd $(DESTDIR)/usr/lib/windows-gaming/ovmf-vars.fd
	install -D -m644 $(GA_ISO) $(DESTDIR)/usr/lib/windows-gaming/windows-gaming-ga.iso
	install -D -m644 ../virtio-win_amd64.vfd $(DESTDIR)/usr/lib/windows-gaming/virtio-win.vfd #FIXME
	install -D -m644 misc/windows.service $(DESTDIR)/lib/systemd/system/windows.service
	install -D -m644 misc/windows.service $(DESTDIR)/lib/systemd/user/windows.service
	install -D -m644 misc/80-vfio.rules $(DESTDIR)/lib/udev/rules.d/80-vfio.rules
	install -D -m644 misc/logind.conf $(DESTDIR)/lib/systemd/logind.conf.d/windows-gaming.conf



.PHONY: OVMF clean all install cargo
